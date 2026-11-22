// SPDX-License-Identifier: GPL-3.0-or-later
// The node/route/metadata model and the cubic volume scale follow wiremix
// (https://github.com/tsowell/wiremix, Copyright Thomas Sowell, MIT OR Apache-2.0), see CREDITS.md.
//! The audio graph as the mixer sees it, and every decision of the mixer, with no PipeWire in it.
//!
//! The PipeWire thread ([`super::pw`]) feeds server events in ([`Graph::add_node`],
//! [`Graph::node_params`], [`Graph::metadata`], ...); D-Bus calls ask it for lists and for the
//! [`Op`]s that carry out a request ([`Graph::set_volume`], [`Graph::set_default`], ...). Tests drive
//! it with hand-made node graphs and check the ops, so the rules (cubic volume, routes of ALSA
//! devices, default switching, per-app memory) are tested without an audio server.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

use desk_proto::{MixerApp, MixerDevice, MixerStream};

/// Loudest volume a slider can ask for (150 %, GNOME's over-amplification limit).
pub const MAX_VOLUME: f64 = 1.5;
/// How long a paused stream stays in the application list.
pub const LINGER: Duration = Duration::from_secs(30);

/// What changed, for the coalesced `Changed` signal.
pub mod dirty {
    pub const DEVICES: u8 = 1;
    pub const APPS: u8 = 2;
    pub const DEFAULTS: u8 = 4;
    pub const AVAILABLE: u8 = 8;
    pub const ALL: u8 = DEVICES | APPS | DEFAULTS | AVAILABLE;

    /// The names `Changed` carries (desk_proto::mixer::CHANGED_*).
    pub fn names(flags: u8) -> Vec<String> {
        use desk_proto::mixer::*;
        [
            (DEVICES, CHANGED_DEVICES),
            (APPS, CHANGED_APPS),
            (DEFAULTS, CHANGED_DEFAULTS),
            (AVAILABLE, CHANGED_AVAILABLE),
        ]
        .into_iter()
        .filter(|(f, _)| flags & f != 0)
        .map(|(_, n)| n.to_string())
        .collect()
    }
}

// ------------------------------------------------------------------ volume scale

/// Slider position (cubic, 1.0 = 100 %) of a linear channel volume, as wpctl and pulseaudio show it.
pub fn to_cubic(linear: f32) -> f64 {
    f64::from(linear.max(0.0)).cbrt()
}

/// Linear channel volume for a slider position, clamped to `0 ..= MAX_VOLUME`.
pub fn to_linear(cubic: f64) -> f32 {
    let c = if cubic.is_finite() { cubic } else { 0.0 };
    c.clamp(0.0, MAX_VOLUME).powi(3) as f32
}

/// Slider position of a set of channel volumes: the loudest channel (as pulseaudio does).
pub fn level(volumes: &[f32]) -> f64 {
    to_cubic(volumes.iter().copied().fold(0.0, f32::max))
}

/// Channel volumes for a new slider position that keep the balance between channels.
pub fn scaled(volumes: &[f32], cubic: f64) -> Vec<f32> {
    let target = to_linear(cubic);
    let max = volumes.iter().copied().fold(0.0, f32::max);
    if max <= f32::EPSILON {
        vec![target; volumes.len()]
    } else {
        volumes.iter().map(|v| v / max * target).collect()
    }
}

// ------------------------------------------------------------------ the graph

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// `Audio/Sink`: an output device.
    Sink,
    /// `Audio/Source`: an input device.
    Source,
    /// `Stream/Output/Audio`: an application playing.
    Stream,
}

impl Class {
    pub fn from_media_class(c: &str) -> Option<Class> {
        match c {
            "Audio/Sink" => Some(Class::Sink),
            "Audio/Source" => Some(Class::Source),
            "Stream/Output/Audio" => Some(Class::Stream),
            _ => None,
        }
    }
}

/// The node properties the mixer reads (`node.name`, `application.name`, ...), copied out of
/// PipeWire's dictionaries.
pub type Props = BTreeMap<String, String>;

/// The property keys worth copying (everything else is ignored).
pub const PROP_KEYS: &[&str] = &[
    "media.class",
    "node.name",
    "node.description",
    "node.nick",
    "media.name",
    "media.role",
    "device.id",
    "device.icon-name",
    "device.bus",
    "device.api",
    "card.profile.device",
    "application.name",
    "application.id",
    "application.icon-name",
    "application.process.binary",
    "pipewire.access.portal.app_id",
    "stream.monitor",
    "object.serial",
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub id: u32,
    pub class: Option<Class>,
    pub props: Props,
    pub volumes: Vec<f32>,
    pub mute: bool,
    /// The volume is known (the first `Props` param arrived).
    pub has_volume: bool,
    pub running: bool,
    /// When a stream last stopped running (for [`LINGER`]).
    pub stopped_at: Option<Instant>,
}

impl Node {
    fn prop(&self, k: &str) -> &str {
        self.props.get(k).map(String::as_str).unwrap_or("")
    }

    fn first(&self, keys: &[&str]) -> &str {
        keys.iter()
            .map(|k| self.prop(k))
            .find(|v| !v.is_empty())
            .unwrap_or("")
    }

    pub fn name(&self) -> &str {
        self.prop("node.name")
    }

    /// Stable key of the application a stream belongs to.
    pub fn app_key(&self) -> String {
        let k = self.first(&[
            "pipewire.access.portal.app_id",
            "application.id",
            "application.process.binary",
            "application.name",
            "node.name",
        ]);
        if k.is_empty() {
            format!("node-{}", self.id)
        } else {
            k.to_string()
        }
    }

    pub fn app_name(&self) -> String {
        let n = self.first(&[
            "application.name",
            "application.process.binary",
            "node.description",
            "node.name",
        ]);
        if n.is_empty() {
            self.app_key()
        } else {
            n.to_string()
        }
    }

    /// Desktop entry ids that may describe the application (for its icon).
    pub fn desktop_candidates(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for k in [
            "pipewire.access.portal.app_id",
            "application.id",
            "application.process.binary",
            "application.name",
        ] {
            let v = self.prop(k);
            if v.is_empty() {
                continue;
            }
            for c in [v.to_string(), v.to_lowercase()] {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
        out
    }

    pub fn description(&self) -> String {
        self.first(&["node.description", "node.nick", "node.name"])
            .to_string()
    }

    /// `device.id` and `card.profile.device`: where an ALSA node keeps its volume (a device route).
    pub fn route_key(&self) -> Option<(u32, i32)> {
        Some((
            self.prop("device.id").parse().ok()?,
            self.prop("card.profile.device").parse().ok()?,
        ))
    }

    pub fn bus(&self) -> &'static str {
        match (self.prop("device.api"), self.prop("device.bus")) {
            ("bluez5", _) | (_, "bluetooth") => "bluetooth",
            (_, "usb") => "usb",
            (_, "pci") => "pci",
            _ => "",
        }
    }

    /// Streams the user never wants in the mixer: monitors (level meters of other mixers) and the
    /// event sounds of the shell.
    fn hidden_stream(&self) -> bool {
        self.prop("stream.monitor") == "true"
            || (self.prop("media.role") == "event"
                && (self.prop("application.process.binary") == "gnome-shell"
                    || self.prop("application.id") == "org.gnome.Shell"))
    }
}

/// A device route (ALSA port) that is in use, with its own volume.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub index: i32,
    /// `card.profile.device` of the nodes it carries.
    pub device: i32,
    pub volumes: Vec<f32>,
    pub mute: bool,
}

/// A request for the audio server, carried out by the PipeWire thread (or recorded by tests).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    NodeVolumes {
        node: u32,
        volumes: Vec<f32>,
    },
    NodeMute {
        node: u32,
        mute: bool,
    },
    /// Volume or mute of a device route (what pavucontrol and wpctl change for ALSA devices, so that
    /// the hardware mixer is used and the value is saved per port).
    Route {
        device: u32,
        index: i32,
        card_device: i32,
        volumes: Option<Vec<f32>>,
        mute: Option<bool>,
    },
    /// A property of the `default` metadata object; `value: None` deletes the key.
    Metadata {
        subject: u32,
        key: String,
        type_: Option<String>,
        value: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MixError {
    Unavailable,
    NoSuchNode(u32),
    NoSuchApp(String),
    /// The node exists but is not what the call needs (a stream given as a device, ...).
    WrongKind(u32),
    /// The server has not told us the volume yet (channel count unknown).
    VolumeUnknown(u32),
    BadKind(String),
}

impl std::fmt::Display for MixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MixError::Unavailable => write!(f, "the audio server is not available"),
            MixError::NoSuchNode(id) => write!(f, "no device or stream {id}"),
            MixError::NoSuchApp(a) => write!(f, "no application {a}"),
            MixError::WrongKind(id) => write!(f, "{id} is not a device of that kind"),
            MixError::VolumeUnknown(id) => write!(f, "the volume of {id} is not known yet"),
            MixError::BadKind(k) => write!(f, "unknown kind {k:?} (output or input)"),
        }
    }
}

/// Volume and mute the user gave an application, restored when its stream comes back.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Remembered {
    pub volume: f64,
    pub mute: bool,
}

/// Settings of the engine (GSettings `io.github.OlehDzhumyk.Desk.Mixer`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    pub remember: bool,
    pub hide_apps: Vec<String>,
}

pub type IconResolver = Box<dyn Fn(&[String]) -> Option<String> + Send>;

pub struct Graph {
    pub connected: bool,
    pub cfg: Config,
    nodes: BTreeMap<u32, Node>,
    /// device id → its routes in use
    routes: HashMap<u32, Vec<Route>>,
    /// link id → (output node, input node)
    links: HashMap<u32, (u32, u32)>,
    /// id of the `default` metadata object
    pub metadata_id: Option<u32>,
    default_sink: Option<String>,
    default_source: Option<String>,
    /// streams with an explicit target in the metadata: stream → (`target.node` /
    /// `target.object` → value)
    targets: HashMap<u32, BTreeMap<String, String>>,
    /// per-app memory (GSettings remember-app-volume)
    pub memory: BTreeMap<String, Remembered>,
    restored: HashSet<u32>,
    icons: HashMap<String, String>,
    resolve_icon: IconResolver,
}

impl Default for Graph {
    fn default() -> Self {
        Graph::new(Config::default(), Box::new(|_| None))
    }
}

fn clamp(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(0.0, MAX_VOLUME)
    } else {
        0.0
    }
}

fn parse_kind(kind: &str) -> Result<Class, MixError> {
    match kind {
        desk_proto::mixer::OUTPUT => Ok(Class::Sink),
        desk_proto::mixer::INPUT => Ok(Class::Source),
        _ => Err(MixError::BadKind(kind.to_string())),
    }
}

/// The node name in a `default.audio.sink` value (`{"name": "..."}`).
fn json_name(value: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(value).ok()?;
    v.get("name")?.as_str().map(str::to_string)
}

impl Graph {
    pub fn new(cfg: Config, resolve_icon: IconResolver) -> Graph {
        Graph {
            connected: false,
            cfg,
            nodes: BTreeMap::new(),
            routes: HashMap::new(),
            links: HashMap::new(),
            metadata_id: None,
            default_sink: None,
            default_source: None,
            targets: HashMap::new(),
            memory: BTreeMap::new(),
            restored: HashSet::new(),
            icons: HashMap::new(),
            resolve_icon,
        }
    }

    /// The server went away: forget its objects (ids are reused by the next server).
    pub fn reset(&mut self) {
        self.connected = false;
        self.nodes.clear();
        self.routes.clear();
        self.links.clear();
        self.metadata_id = None;
        self.default_sink = None;
        self.default_source = None;
        self.targets.clear();
        self.restored.clear();
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(&id)
    }

    // -------------------------------------------------------------- server events

    /// A node appeared or its properties changed. Returns the dirty flags.
    pub fn add_node(&mut self, id: u32, props: Props) -> u8 {
        let Some(class) = props.get("media.class").and_then(|c| Class::from_media_class(c)) else {
            return 0;
        };
        let n = self.nodes.entry(id).or_insert_with(|| Node {
            id,
            ..Node::default()
        });
        if n.class == Some(class) && n.props == props {
            return 0;
        }
        n.class = Some(class);
        n.props = props;
        match class {
            Class::Stream => dirty::APPS,
            _ => dirty::DEVICES | dirty::DEFAULTS,
        }
    }

    /// The node's state changed (running or not).
    pub fn node_state(&mut self, id: u32, running: bool, now: Instant) -> u8 {
        let Some(n) = self.nodes.get_mut(&id) else {
            return 0;
        };
        if n.running == running {
            return 0;
        }
        n.running = running;
        n.stopped_at = (!running).then_some(now);
        if n.class == Some(Class::Stream) {
            dirty::APPS
        } else {
            0
        }
    }

    /// A `Props` param of a node: channel volumes and/or mute. Returns the dirty flags and the ops
    /// that restore a remembered application volume when a stream's volume becomes known.
    pub fn node_params(
        &mut self,
        id: u32,
        volumes: Option<Vec<f32>>,
        mute: Option<bool>,
    ) -> (u8, Vec<Op>) {
        let Some(n) = self.nodes.get_mut(&id) else {
            return (0, vec![]);
        };
        let before = (n.volumes.clone(), n.mute, n.has_volume);
        if let Some(v) = volumes
            && !v.is_empty()
        {
            n.volumes = v;
            n.has_volume = true;
        }
        if let Some(m) = mute {
            n.mute = m;
        }
        if before == (n.volumes.clone(), n.mute, n.has_volume) {
            return (0, vec![]);
        }
        let flags = match n.class {
            Some(Class::Stream) => dirty::APPS,
            _ => dirty::DEVICES,
        };
        (flags, self.restore(id))
    }

    /// Ops that put back the remembered volume of a stream's application (once per stream).
    fn restore(&mut self, id: u32) -> Vec<Op> {
        let Some(n) = self.nodes.get(&id) else {
            return vec![];
        };
        if !self.cfg.remember
            || n.class != Some(Class::Stream)
            || !n.has_volume
            || self.restored.contains(&id)
        {
            return vec![];
        }
        let Some(r) = self.memory.get(&n.app_key()).copied() else {
            return vec![];
        };
        self.restored.insert(id);
        let mut ops = Vec::new();
        if (level(&n.volumes) - r.volume).abs() > 0.005 {
            ops.push(Op::NodeVolumes {
                node: id,
                volumes: scaled(&n.volumes, r.volume),
            });
        }
        if n.mute != r.mute {
            ops.push(Op::NodeMute {
                node: id,
                mute: r.mute,
            });
        }
        ops
    }

    /// A `Route` param of a device (an active route with its volume).
    pub fn device_route(&mut self, device: u32, route: Route) -> u8 {
        let routes = self.routes.entry(device).or_default();
        match routes.iter_mut().find(|r| r.device == route.device) {
            Some(r) if *r == route => return 0,
            Some(r) => *r = route,
            None => routes.push(route),
        }
        dirty::DEVICES
    }

    pub fn add_link(&mut self, id: u32, output: u32, input: u32) -> u8 {
        if self.links.insert(id, (output, input)) == Some((output, input)) {
            return 0;
        }
        if self.nodes.get(&output).and_then(|n| n.class) == Some(Class::Stream) {
            dirty::APPS
        } else {
            0
        }
    }

    /// Any global object went away.
    pub fn remove(&mut self, id: u32) -> u8 {
        if let Some(n) = self.nodes.remove(&id) {
            self.restored.remove(&id);
            self.targets.remove(&id);
            return match n.class {
                Some(Class::Stream) => dirty::APPS,
                _ => dirty::DEVICES | dirty::DEFAULTS,
            };
        }
        if let Some((out, _)) = self.links.remove(&id) {
            return if self.nodes.contains_key(&out) {
                dirty::APPS
            } else {
                0
            };
        }
        if self.routes.remove(&id).is_some() {
            return dirty::DEVICES;
        }
        if self.metadata_id == Some(id) {
            self.metadata_id = None;
            self.default_sink = None;
            self.default_source = None;
            self.targets.clear();
            return dirty::DEFAULTS | dirty::DEVICES;
        }
        0
    }

    /// A property of the `default` metadata. `key: None` clears every key of `subject`.
    pub fn metadata(&mut self, subject: u32, key: Option<&str>, value: Option<&str>) -> u8 {
        if subject == 0 {
            let name = value.and_then(json_name);
            let (sink, source) = match key {
                Some("default.audio.sink") => (Some(name), None),
                Some("default.audio.source") => (None, Some(name)),
                None => (Some(None), Some(None)),
                _ => (None, None),
            };
            let mut changed = false;
            if let Some(s) = sink
                && self.default_sink != s
            {
                self.default_sink = s;
                changed = true;
            }
            if let Some(s) = source
                && self.default_source != s
            {
                self.default_source = s;
                changed = true;
            }
            return if changed {
                dirty::DEFAULTS | dirty::DEVICES
            } else {
                0
            };
        }
        let before = self.targets.get(&subject).cloned();
        match key {
            None => {
                self.targets.remove(&subject);
            }
            Some(k @ ("target.node" | "target.object")) => {
                let t = self.targets.entry(subject).or_default();
                match value.filter(|v| !v.is_empty() && *v != "-1") {
                    Some(v) => {
                        t.insert(k.to_string(), v.to_string());
                    }
                    None => {
                        t.remove(k);
                    }
                }
                if t.is_empty() {
                    self.targets.remove(&subject);
                }
            }
            _ => return 0,
        }
        if before == self.targets.get(&subject).cloned() {
            0
        } else {
            dirty::APPS
        }
    }

    // -------------------------------------------------------------- queries

    fn by_name(&self, class: Class, name: &Option<String>) -> Option<u32> {
        let name = name.as_deref()?;
        self.nodes
            .values()
            .find(|n| n.class == Some(class) && n.name() == name)
            .map(|n| n.id)
    }

    pub fn default_id(&self, class: Class) -> Option<u32> {
        match class {
            Class::Sink => self.by_name(Class::Sink, &self.default_sink),
            Class::Source => self.by_name(Class::Source, &self.default_source),
            Class::Stream => None,
        }
    }

    /// The route that carries a device node's volume, if it has one.
    fn route_of(&self, n: &Node) -> Option<(u32, &Route)> {
        let (dev, card_device) = n.route_key()?;
        let r = self.routes.get(&dev)?.iter().find(|r| r.device == card_device)?;
        Some((dev, r))
    }

    /// Volume and mute as the user sees them: the route's for ALSA devices, else the node's.
    fn shown(&self, n: &Node) -> (f64, bool) {
        match self.route_of(n) {
            Some((_, r)) if !r.volumes.is_empty() => (level(&r.volumes), r.mute),
            _ => (level(&n.volumes), n.mute),
        }
    }

    /// Output device a stream is linked to (0 = none yet).
    fn sink_of(&self, stream: u32) -> u32 {
        self.links
            .values()
            .filter(|(out, _)| *out == stream)
            .map(|(_, input)| *input)
            .find(|i| self.nodes.get(i).and_then(|n| n.class) == Some(Class::Sink))
            .unwrap_or(0)
    }

    fn icon(&mut self, n: &Node) -> String {
        let named = n.prop("application.icon-name");
        if !named.is_empty() {
            return named.to_string();
        }
        let key = n.app_key();
        if let Some(i) = self.icons.get(&key) {
            return i.clone();
        }
        let icon = (self.resolve_icon)(&n.desktop_candidates()).unwrap_or_default();
        self.icons.insert(key, icon.clone());
        icon
    }

    pub fn devices(&self, kind: &str) -> Result<Vec<MixerDevice>, MixError> {
        let class = parse_kind(kind)?;
        let default = self.default_id(class);
        let mut out: Vec<MixerDevice> = self
            .nodes
            .values()
            .filter(|n| n.class == Some(class))
            .map(|n| {
                let (volume, mute) = self.shown(n);
                MixerDevice {
                    id: n.id,
                    name: n.name().to_string(),
                    description: n.description(),
                    icon: n.prop("device.icon-name").to_string(),
                    bus: n.bus().to_string(),
                    volume,
                    mute,
                    is_default: Some(n.id) == default,
                }
            })
            .collect();
        out.sort_by(|a, b| a.description.cmp(&b.description).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    fn visible_streams(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(|n| {
            n.class == Some(Class::Stream)
                && !n.hidden_stream()
                && !self.cfg.hide_apps.iter().any(|h| *h == n.app_key())
        })
    }

    pub fn streams(&mut self) -> Vec<MixerStream> {
        let nodes: Vec<Node> = self.visible_streams().cloned().collect();
        nodes
            .iter()
            .map(|n| MixerStream {
                id: n.id,
                app: n.app_key(),
                name: n.app_name(),
                icon: self.icon(n),
                media: n.prop("media.name").to_string(),
                volume: level(&n.volumes),
                mute: n.mute,
                playing: n.running,
                device: self.sink_of(n.id),
            })
            .collect()
    }

    /// A stream is listed while it plays, for [`LINGER`] after it stopped, or always.
    fn listed(n: &Node, now: Instant, include_silent: bool) -> bool {
        include_silent
            || n.running
            || n.stopped_at
                .is_some_and(|t| now.saturating_duration_since(t) < LINGER)
    }

    pub fn apps(&mut self, now: Instant, include_silent: bool) -> Vec<MixerApp> {
        let streams: Vec<Node> = self
            .visible_streams()
            .filter(|n| Self::listed(n, now, include_silent))
            .cloned()
            .collect();
        let mut apps: BTreeMap<String, MixerApp> = BTreeMap::new();
        for n in &streams {
            let icon = self.icon(n);
            let sink = self.sink_of(n.id);
            let level = level(&n.volumes);
            let a = apps.entry(n.app_key()).or_insert_with(|| MixerApp {
                key: n.app_key(),
                name: n.app_name(),
                icon,
                media: String::new(),
                volume: 0.0,
                mute: true,
                playing: false,
                device: sink,
                streams: vec![],
            });
            a.volume = a.volume.max(level);
            a.mute &= n.mute;
            if n.running && !a.playing || a.media.is_empty() {
                a.media = n.prop("media.name").to_string();
            }
            a.playing |= n.running;
            if a.device == 0 {
                a.device = sink;
            }
            a.streams.push(n.id);
        }
        let mut out: Vec<MixerApp> = apps.into_values().collect();
        out.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then(a.key.cmp(&b.key))
        });
        out
    }

    /// When the next paused stream leaves the list (so `Changed` can be sent then, and only then).
    pub fn next_expiry(&self, now: Instant) -> Option<Instant> {
        self.visible_streams()
            .filter(|n| !n.running)
            .filter_map(|n| n.stopped_at.map(|t| t + LINGER))
            .filter(|t| *t > now)
            .min()
    }

    // -------------------------------------------------------------- requests

    fn need_connected(&self) -> Result<(), MixError> {
        if self.connected {
            Ok(())
        } else {
            Err(MixError::Unavailable)
        }
    }

    fn get(&self, id: u32) -> Result<&Node, MixError> {
        self.need_connected()?;
        self.nodes.get(&id).ok_or(MixError::NoSuchNode(id))
    }

    fn app_streams(&self, app: &str) -> Result<Vec<u32>, MixError> {
        self.need_connected()?;
        let ids: Vec<u32> = self
            .nodes
            .values()
            .filter(|n| n.class == Some(Class::Stream) && n.app_key() == app)
            .map(|n| n.id)
            .collect();
        if ids.is_empty() {
            Err(MixError::NoSuchApp(app.to_string()))
        } else {
            Ok(ids)
        }
    }

    /// Remember what the user chose for an application. Returns whether the memory changed.
    fn remember(&mut self, app: &str, volume: Option<f64>, mute: Option<bool>) -> bool {
        if !self.cfg.remember {
            return false;
        }
        let cur = self.memory.get(app).copied();
        let base = cur.unwrap_or_else(|| {
            let ids = self.app_streams(app).unwrap_or_default();
            let ns: Vec<&Node> = ids.iter().filter_map(|i| self.nodes.get(i)).collect();
            Remembered {
                volume: ns.iter().map(|n| level(&n.volumes)).fold(0.0, f64::max),
                mute: !ns.is_empty() && ns.iter().all(|n| n.mute),
            }
        });
        let new = Remembered {
            volume: volume.map(clamp).unwrap_or(base.volume),
            mute: mute.unwrap_or(base.mute),
        };
        if cur == Some(new) {
            return false;
        }
        self.memory.insert(app.to_string(), new);
        true
    }

    fn volume_op(&self, n: &Node, volume: f64) -> Result<Op, MixError> {
        if let Some((device, r)) = self.route_of(n)
            && !r.volumes.is_empty()
        {
            return Ok(Op::Route {
                device,
                index: r.index,
                card_device: r.device,
                volumes: Some(scaled(&r.volumes, volume)),
                mute: None,
            });
        }
        if !n.has_volume {
            return Err(MixError::VolumeUnknown(n.id));
        }
        Ok(Op::NodeVolumes {
            node: n.id,
            volumes: scaled(&n.volumes, volume),
        })
    }

    fn mute_op(&self, n: &Node, mute: bool) -> Op {
        match self.route_of(n) {
            Some((device, r)) => Op::Route {
                device,
                index: r.index,
                card_device: r.device,
                volumes: None,
                mute: Some(mute),
            },
            None => Op::NodeMute { node: n.id, mute },
        }
    }

    /// Ops for `SetVolume(id, volume)`; the bool says whether the per-app memory changed.
    pub fn set_volume(&mut self, id: u32, volume: f64) -> Result<(Vec<Op>, bool), MixError> {
        let n = self.get(id)?;
        let op = self.volume_op(n, clamp(volume))?;
        let app = (n.class == Some(Class::Stream)).then(|| n.app_key());
        let remembered = app.is_some_and(|a| self.remember(&a, Some(volume), None));
        Ok((vec![op], remembered))
    }

    pub fn set_mute(&mut self, id: u32, mute: bool) -> Result<(Vec<Op>, bool), MixError> {
        let n = self.get(id)?;
        let op = self.mute_op(n, mute);
        let app = (n.class == Some(Class::Stream)).then(|| n.app_key());
        let remembered = app.is_some_and(|a| self.remember(&a, None, Some(mute)));
        Ok((vec![op], remembered))
    }

    pub fn set_app_volume(&mut self, app: &str, volume: f64) -> Result<(Vec<Op>, bool), MixError> {
        let ids = self.app_streams(app)?;
        let mut ops = Vec::new();
        for id in ids {
            let n = &self.nodes[&id];
            if n.has_volume {
                ops.push(self.volume_op(n, clamp(volume))?);
            }
        }
        let remembered = self.remember(app, Some(volume), None);
        Ok((ops, remembered))
    }

    pub fn set_app_mute(&mut self, app: &str, mute: bool) -> Result<(Vec<Op>, bool), MixError> {
        let ids = self.app_streams(app)?;
        let ops = ids
            .iter()
            .map(|id| self.mute_op(&self.nodes[id], mute))
            .collect();
        let remembered = self.remember(app, None, Some(mute));
        Ok((ops, remembered))
    }

    /// Ops that make a stream follow the default device again.
    fn follow_default(stream: u32) -> [Op; 2] {
        ["target.object", "target.node"].map(|key| Op::Metadata {
            subject: stream,
            key: key.to_string(),
            type_: Some("Spa:Id".to_string()),
            value: Some("-1".to_string()),
        })
    }

    pub fn set_default(&mut self, kind: &str, id: u32) -> Result<Vec<Op>, MixError> {
        let class = parse_kind(kind)?;
        let n = self.get(id)?;
        if n.class != Some(class) {
            return Err(MixError::WrongKind(id));
        }
        let key = match class {
            Class::Sink => "default.configured.audio.sink",
            _ => "default.configured.audio.source",
        };
        let mut ops = vec![Op::Metadata {
            subject: 0,
            key: key.to_string(),
            type_: Some("Spa:String:JSON".to_string()),
            value: Some(serde_json::json!({ "name": n.name() }).to_string()),
        }];
        // "selecting makes it default and moves all playing streams to it": streams without a
        // target follow the default by themselves, the ones moved by hand are released
        if class == Class::Sink {
            let mut moved: Vec<u32> = self
                .targets
                .keys()
                .copied()
                .filter(|s| self.nodes.get(s).and_then(|n| n.class) == Some(Class::Stream))
                .collect();
            moved.sort_unstable();
            for s in moved {
                ops.extend(Self::follow_default(s));
            }
        }
        Ok(ops)
    }

    pub fn move_stream(&self, stream: u32, device: u32) -> Result<Vec<Op>, MixError> {
        let n = self.get(stream)?;
        if n.class != Some(Class::Stream) {
            return Err(MixError::WrongKind(stream));
        }
        if device == 0 {
            return Ok(Self::follow_default(stream).to_vec());
        }
        if self.get(device)?.class != Some(Class::Sink) {
            return Err(MixError::WrongKind(device));
        }
        Ok(vec![
            Op::Metadata {
                subject: stream,
                key: "target.object".to_string(),
                type_: None,
                value: None,
            },
            Op::Metadata {
                subject: stream,
                key: "target.node".to_string(),
                type_: Some("Spa:Id".to_string()),
                value: Some(device.to_string()),
            },
        ])
    }

    pub fn move_app(&self, app: &str, device: u32) -> Result<Vec<Op>, MixError> {
        let mut ops = Vec::new();
        for s in self.app_streams(app)? {
            ops.extend(self.move_stream(s, device)?);
        }
        Ok(ops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props(kv: &[(&str, &str)]) -> Props {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn sink(name: &str, desc: &str) -> Props {
        props(&[
            ("media.class", "Audio/Sink"),
            ("node.name", name),
            ("node.description", desc),
        ])
    }

    fn stream(app: &str, binary: &str) -> Props {
        props(&[
            ("media.class", "Stream/Output/Audio"),
            ("application.name", app),
            ("application.process.binary", binary),
            ("media.name", "Playback"),
        ])
    }

    /// Two null sinks (30, 31), a source (40), Firefox with two streams (50, 51) and mpv (60).
    fn graph(remember: bool) -> Graph {
        let mut g = Graph::new(
            Config {
                remember,
                hide_apps: vec![],
            },
            Box::new(|c| c.first().map(|c| format!("{c}-icon"))),
        );
        g.connected = true;
        g.metadata_id = Some(5);
        g.add_node(30, sink("speakers", "Speakers"));
        g.add_node(31, sink("headphones", "Headphones"));
        g.add_node(
            40,
            props(&[
                ("media.class", "Audio/Source"),
                ("node.name", "mic"),
                ("node.description", "Microphone"),
            ]),
        );
        for (id, p) in [
            (50, stream("Firefox", "firefox")),
            (51, stream("Firefox", "firefox")),
            (60, stream("mpv", "mpv")),
        ] {
            g.add_node(id, p);
            g.node_params(id, Some(vec![1.0, 1.0]), Some(false));
            g.node_state(id, true, Instant::now());
        }
        for id in [30, 31, 40] {
            g.node_params(id, Some(vec![0.125, 0.125]), Some(false));
        }
        g.metadata(0, Some("default.audio.sink"), Some(r#"{"name":"speakers"}"#));
        g.add_link(100, 50, 30);
        g.add_link(101, 60, 31);
        g
    }

    #[test]
    fn cubic_scale_matches_wpctl() {
        // wpctl set-volume 0.5 stores 0.125 on every channel
        assert_eq!(to_linear(0.5), 0.125);
        assert!((to_cubic(0.125) - 0.5).abs() < 1e-9);
        assert_eq!(to_linear(2.0), to_linear(MAX_VOLUME));
        assert_eq!(to_linear(-1.0), 0.0);
        assert_eq!(to_linear(f64::NAN), 0.0);
        // the balance between channels survives a volume change
        let v = scaled(&[0.5, 0.25], 0.5);
        assert!((v[0] - 0.125).abs() < 1e-6 && (v[1] - 0.0625).abs() < 1e-6, "{v:?}");
        assert_eq!(scaled(&[0.0, 0.0], 1.0), vec![1.0, 1.0]);
        assert!((level(&[0.125, 0.001]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn apps_merge_streams_and_follow_links() {
        let mut g = graph(false);
        let apps = g.apps(Instant::now(), false);
        assert_eq!(apps.len(), 2);
        let ff = &apps[0];
        assert_eq!((ff.key.as_str(), ff.name.as_str()), ("firefox", "Firefox"));
        assert_eq!(ff.streams, vec![50, 51]);
        assert_eq!(ff.device, 30);
        assert_eq!(ff.icon, "firefox-icon");
        assert_eq!(apps[1].device, 31);
        // a Flatpak app id wins over the binary name
        let mut p = stream("Firefox", "firefox");
        p.insert(
            "pipewire.access.portal.app_id".into(),
            "org.mozilla.firefox".into(),
        );
        g.add_node(52, p);
        assert!(g.apps(Instant::now(), true).iter().any(|a| a.key == "org.mozilla.firefox"));
    }

    #[test]
    fn hidden_streams_are_not_listed() {
        let mut g = graph(false);
        let mut ev = stream("GNOME Shell", "gnome-shell");
        ev.insert("media.role".into(), "event".into());
        g.add_node(70, ev);
        let mut mon = stream("pavucontrol", "pavucontrol");
        mon.insert("stream.monitor".into(), "true".into());
        g.add_node(71, mon);
        g.cfg.hide_apps = vec!["mpv".into()];
        let keys: Vec<String> = g.apps(Instant::now(), true).into_iter().map(|a| a.key).collect();
        assert_eq!(keys, vec!["firefox"]);
    }

    #[test]
    fn paused_streams_linger_then_leave() {
        let mut g = graph(false);
        let t0 = Instant::now();
        assert_eq!(g.node_state(60, false, t0), dirty::APPS);
        assert_eq!(g.next_expiry(t0), Some(t0 + LINGER));
        let names = |g: &mut Graph, t, silent| -> Vec<String> {
            g.apps(t, silent).into_iter().map(|a| a.key).collect()
        };
        assert_eq!(names(&mut g, t0 + Duration::from_secs(29), false), ["firefox", "mpv"]);
        assert_eq!(names(&mut g, t0 + LINGER, false), ["firefox"]);
        assert_eq!(names(&mut g, t0 + LINGER, true), ["firefox", "mpv"]);
        assert_eq!(g.next_expiry(t0 + LINGER), None);
    }

    #[test]
    fn app_volume_sets_every_stream_and_nothing_else() {
        let mut g = graph(false);
        let (ops, remembered) = g.set_app_volume("firefox", 0.3).unwrap();
        let v = to_linear(0.3);
        assert_eq!(
            ops,
            vec![
                Op::NodeVolumes { node: 50, volumes: vec![v, v] },
                Op::NodeVolumes { node: 51, volumes: vec![v, v] },
            ]
        );
        assert!(!remembered);
        assert_eq!(g.set_app_volume("nope", 0.3), Err(MixError::NoSuchApp("nope".into())));
        g.connected = false;
        assert_eq!(g.set_volume(30, 0.3), Err(MixError::Unavailable));
    }

    #[test]
    fn alsa_devices_change_the_route_not_the_node() {
        let mut g = graph(false);
        let mut p = sink("alsa_output.pci", "Built-in Audio");
        p.insert("device.id".into(), "7".into());
        p.insert("card.profile.device".into(), "3".into());
        g.add_node(32, p);
        g.node_params(32, Some(vec![1.0, 1.0]), Some(false));
        // no route yet: the node itself
        assert_eq!(
            g.set_mute(32, true).unwrap().0,
            vec![Op::NodeMute { node: 32, mute: true }]
        );
        let flags = g.device_route(
            7,
            Route {
                index: 2,
                device: 3,
                volumes: vec![0.008, 0.008],
                mute: false,
            },
        );
        assert_eq!(flags, dirty::DEVICES);
        let d = g.devices("output").unwrap();
        let bi = d.iter().find(|d| d.id == 32).unwrap();
        assert!((bi.volume - 0.2).abs() < 1e-6, "route volume shown: {}", bi.volume);
        assert_eq!(
            g.set_volume(32, 0.5).unwrap().0,
            vec![Op::Route {
                device: 7,
                index: 2,
                card_device: 3,
                volumes: Some(vec![0.125, 0.125]),
                mute: None,
            }]
        );
        assert!(matches!(g.set_mute(32, true).unwrap().0[0], Op::Route { mute: Some(true), .. }));
    }

    #[test]
    fn default_device_switching() {
        let mut g = graph(false);
        let outs = g.devices("output").unwrap();
        assert_eq!(
            outs.iter().map(|d| (d.id, d.is_default)).collect::<Vec<_>>(),
            vec![(31, false), (30, true)]
        );
        // mpv was moved to the headphones by hand; firefox follows the default
        assert_eq!(g.metadata(60, Some("target.node"), Some("31")), dirty::APPS);
        let ops = g.set_default("output", 31).unwrap();
        assert_eq!(
            ops[0],
            Op::Metadata {
                subject: 0,
                key: "default.configured.audio.sink".into(),
                type_: Some("Spa:String:JSON".into()),
                value: Some(r#"{"name":"headphones"}"#.into()),
            }
        );
        assert_eq!(&ops[1..], &Graph::follow_default(60));
        // the server confirms: the effective default changes
        let f = g.metadata(0, Some("default.audio.sink"), Some(r#"{"name":"headphones"}"#));
        assert_eq!(f, dirty::DEFAULTS | dirty::DEVICES);
        assert_eq!(g.default_id(Class::Sink), Some(31));
        // the default device disappears: nothing is default until the server picks another
        g.remove(31);
        assert_eq!(g.default_id(Class::Sink), None);
        g.metadata(0, Some("default.audio.sink"), Some(r#"{"name":"speakers"}"#));
        assert_eq!(g.default_id(Class::Sink), Some(30));
        // inputs: no stream moves, kinds are checked
        assert_eq!(g.set_default("input", 40).unwrap().len(), 1);
        assert_eq!(g.set_default("input", 30), Err(MixError::WrongKind(30)));
        assert_eq!(g.set_default("both", 30), Err(MixError::BadKind("both".into())));
    }

    #[test]
    fn moving_streams() {
        let g = graph(false);
        let ops = g.move_app("firefox", 31).unwrap();
        assert_eq!(ops.len(), 4);
        assert_eq!(
            ops[1],
            Op::Metadata {
                subject: 50,
                key: "target.node".into(),
                type_: Some("Spa:Id".into()),
                value: Some("31".into()),
            }
        );
        assert_eq!(g.move_stream(50, 0).unwrap(), Graph::follow_default(50).to_vec());
        assert_eq!(g.move_stream(50, 40), Err(MixError::WrongKind(40)));
        assert_eq!(g.move_stream(30, 31), Err(MixError::WrongKind(30)));
    }

    #[test]
    fn per_app_memory_is_restored_when_the_stream_comes_back() {
        let mut g = graph(true);
        let (_, remembered) = g.set_app_volume("mpv", 0.4).unwrap();
        assert!(remembered);
        let (_, remembered) = g.set_app_mute("mpv", true).unwrap();
        assert!(remembered);
        assert_eq!(
            g.memory.get("mpv"),
            Some(&Remembered {
                volume: 0.4,
                mute: true
            })
        );
        // same value again: nothing to save
        assert!(!g.set_app_volume("mpv", 0.4).unwrap().1);
        // mpv quits and starts again
        g.remove(60);
        g.add_node(61, stream("mpv", "mpv"));
        // no ops until the server tells the stream's volume (channel count)
        let (_, ops) = g.node_params(61, None, Some(false));
        assert!(ops.is_empty());
        let (_, ops) = g.node_params(61, Some(vec![1.0, 1.0]), None);
        let v = to_linear(0.4);
        assert_eq!(
            ops,
            vec![
                Op::NodeVolumes { node: 61, volumes: vec![v, v] },
                Op::NodeMute { node: 61, mute: true },
            ]
        );
        // only once: the app may change its volume afterwards
        let (_, ops) = g.node_params(61, Some(vec![0.5, 0.5]), None);
        assert!(ops.is_empty());
        // memory off: nothing remembered, nothing restored
        let mut g = graph(false);
        g.memory.insert("mpv".into(), Remembered { volume: 0.1, mute: false });
        g.add_node(62, stream("mpv", "mpv"));
        assert!(g.node_params(62, Some(vec![1.0]), None).1.is_empty());
    }

    #[test]
    fn unchanged_events_are_not_dirty() {
        let mut g = graph(false);
        assert_eq!(g.add_node(30, sink("speakers", "Speakers")), 0);
        assert_eq!(g.node_params(30, Some(vec![0.125, 0.125]), Some(false)).0, 0);
        assert_eq!(g.node_state(50, true, Instant::now()), 0);
        assert_eq!(g.metadata(0, Some("default.audio.sink"), Some(r#"{"name":"speakers"}"#)), 0);
        assert_eq!(g.add_link(100, 50, 30), 0);
        assert_eq!(g.metadata(0, Some("unrelated"), Some("x")), 0);
        assert_eq!(dirty::names(dirty::APPS | dirty::DEFAULTS), ["apps", "defaults"]);
    }

    #[test]
    fn reset_forgets_the_server_but_not_the_memory() {
        let mut g = graph(true);
        g.set_app_volume("mpv", 0.2).unwrap();
        g.reset();
        assert!(!g.connected);
        assert!(g.devices("output").unwrap().is_empty());
        assert!(g.memory.contains_key("mpv"));
    }
}
