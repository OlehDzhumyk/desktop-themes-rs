// SPDX-License-Identifier: GPL-3.0-or-later
// Event handling and the Props/Route parameter encoding are adapted from wiremix
// (https://github.com/tsowell/wiremix, src/wirehose/{session,node,device,metadata,execute}.rs,
// Copyright Thomas Sowell, MIT OR Apache-2.0), see CREDITS.md.
//! The PipeWire side of the mixer: one thread with a PipeWire main loop that binds the nodes,
//! devices and the `default` metadata, turns their events into [`Graph`] updates, and carries out
//! [`Op`]s sent from D-Bus calls. It sleeps in the loop until the server sends something: no timers,
//! no polling, no audio data.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;

use pipewire as pw;
use pw::device::{Device, DeviceListener};
use pw::metadata::{Metadata, MetadataListener};
use pw::node::{Node, NodeChangeMask, NodeListener, NodeState};
use pw::spa;
use pw::types::ObjectType;
use spa::param::ParamType;
use spa::pod::deserialize::PodDeserializer;
use spa::pod::serialize::PodSerializer;
use spa::pod::{Object, Pod, Property, PropertyFlags, Value, ValueArray};
use spa::utils::dict::DictRef;

use super::model::{Op, PROP_KEYS, Props, Route, dirty};
use super::notify::Notifier;
use super::{Backend, SharedGraph, lock};

enum Cmd {
    Exec(Vec<Op>),
    Quit,
}

/// The connection; dropping it closes the connection and joins the thread.
pub struct PwBackend {
    tx: pw::channel::Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
    alive: Arc<AtomicBool>,
}

impl Backend for PwBackend {
    fn exec(&self, ops: Vec<Op>) {
        let _ = self.tx.send(Cmd::Exec(ops));
    }

    fn alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
}

impl Drop for PwBackend {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl PwBackend {
    /// Connect on a new thread. `on_lost` runs (on that thread) when an established connection
    /// ends without being asked to: the server stopped or restarted.
    pub fn spawn(
        graph: SharedGraph,
        notifier: Notifier,
        on_lost: impl FnOnce() + Send + 'static,
    ) -> PwBackend {
        let (tx, rx) = pw::channel::channel::<Cmd>();
        let alive = Arc::new(AtomicBool::new(true));
        let alive2 = alive.clone();
        let thread = std::thread::Builder::new()
            .name("deskd-pipewire".into())
            .spawn(move || {
                let lost = match run(&graph, &notifier, rx) {
                    Ok(lost) => lost,
                    Err(e) => {
                        eprintln!("deskd: mixer: no PipeWire connection: {e}");
                        false
                    }
                };
                let was = {
                    let mut g = lock(&graph);
                    let was = g.connected;
                    g.forget_server();
                    was
                };
                alive2.store(false, Ordering::Release);
                if was {
                    notifier.touch(dirty::ALL);
                }
                if lost {
                    on_lost();
                }
            })
            .expect("spawn the PipeWire thread");
        PwBackend {
            tx,
            thread: Some(thread),
            alive,
        }
    }
}

/// Proxies and listeners that must stay alive while their objects exist.
#[derive(Default)]
struct Objects {
    nodes: HashMap<u32, (Node, NodeListener)>,
    devices: HashMap<u32, (Device, DeviceListener)>,
    metadata: Option<(u32, Metadata, MetadataListener)>,
}

fn copy_props(d: &DictRef) -> Props {
    PROP_KEYS
        .iter()
        .filter_map(|k| d.get(k).map(|v| (k.to_string(), v.to_string())))
        .collect()
}

fn deserialize(param: Option<&Pod>) -> Option<Object> {
    let pod = param?;
    match PodDeserializer::deserialize_any_from(pod.as_bytes())
        .ok()?
        .1
    {
        Value::Object(o) => Some(o),
        _ => None,
    }
}

/// channelVolumes and mute of a Props object.
fn volume_and_mute(props: Vec<Property>) -> (Option<Vec<f32>>, Option<bool>) {
    let (mut volumes, mut mute) = (None, None);
    for p in props {
        match (p.key, p.value) {
            (spa::sys::SPA_PROP_channelVolumes, Value::ValueArray(ValueArray::Float(v))) => {
                volumes = Some(v)
            }
            (spa::sys::SPA_PROP_mute, Value::Bool(m)) => mute = Some(m),
            _ => {}
        }
    }
    (volumes, mute)
}

/// An active route of a device (index, card device, its volume).
fn parse_route(o: Object) -> Option<Route> {
    let (mut index, mut device, mut vm) = (None, None, (None, None));
    for p in o.properties {
        match (p.key, p.value) {
            (spa::sys::SPA_PARAM_ROUTE_index, Value::Int(i)) => index = Some(i),
            (spa::sys::SPA_PARAM_ROUTE_device, Value::Int(d)) => device = Some(d),
            (spa::sys::SPA_PARAM_ROUTE_props, Value::Object(props)) => {
                vm = volume_and_mute(props.properties)
            }
            _ => {}
        }
    }
    Some(Route {
        index: index?,
        device: device?,
        volumes: vm.0.unwrap_or_default(),
        mute: vm.1.unwrap_or(false),
    })
}

fn serialize(value: Value) -> Option<Vec<u8>> {
    PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &value)
        .ok()
        .map(|(c, _)| c.into_inner())
}

fn props_object(id: u32, volumes: Option<Vec<f32>>, mute: Option<bool>) -> Object {
    let mut properties = Vec::new();
    if let Some(v) = volumes {
        properties.push(Property {
            key: spa::sys::SPA_PROP_channelVolumes,
            flags: PropertyFlags::empty(),
            value: Value::ValueArray(ValueArray::Float(v)),
        });
    }
    if let Some(m) = mute {
        properties.push(Property {
            key: spa::sys::SPA_PROP_mute,
            flags: PropertyFlags::empty(),
            value: Value::Bool(m),
        });
    }
    Object {
        type_: spa::sys::SPA_TYPE_OBJECT_Props,
        id,
        properties,
    }
}

fn exec(objs: &Objects, op: Op) {
    match op {
        Op::NodeVolumes { node, volumes } => set_node(objs, node, Some(volumes), None),
        Op::NodeMute { node, mute } => set_node(objs, node, None, Some(mute)),
        Op::Route {
            device,
            index,
            card_device,
            volumes,
            mute,
        } => {
            let Some((d, _)) = objs.devices.get(&device) else {
                return;
            };
            let int = |key, v| Property {
                key,
                flags: PropertyFlags::empty(),
                value: Value::Int(v),
            };
            let route = Value::Object(Object {
                type_: spa::sys::SPA_TYPE_OBJECT_ParamRoute,
                id: spa::sys::SPA_PARAM_Route,
                properties: vec![
                    int(spa::sys::SPA_PARAM_ROUTE_index, index),
                    int(spa::sys::SPA_PARAM_ROUTE_device, card_device),
                    Property {
                        key: spa::sys::SPA_PARAM_ROUTE_props,
                        flags: PropertyFlags::empty(),
                        value: Value::Object(props_object(
                            spa::sys::SPA_PARAM_Route,
                            volumes,
                            mute,
                        )),
                    },
                    // remembered per port by the session manager, as pavucontrol does
                    Property {
                        key: spa::sys::SPA_PARAM_ROUTE_save,
                        flags: PropertyFlags::empty(),
                        value: Value::Bool(true),
                    },
                ],
            });
            if let Some(bytes) = serialize(route)
                && let Some(pod) = Pod::from_bytes(&bytes)
            {
                d.set_param(ParamType::Route, 0, pod);
            }
        }
        Op::Metadata {
            subject,
            key,
            type_,
            value,
        } => {
            if let Some((_, m, _)) = &objs.metadata {
                m.set_property(subject, &key, type_.as_deref(), value.as_deref());
            }
        }
    }
}

fn set_node(objs: &Objects, node: u32, volumes: Option<Vec<f32>>, mute: Option<bool>) {
    let Some((n, _)) = objs.nodes.get(&node) else {
        return;
    };
    let props = Value::Object(props_object(spa::sys::SPA_PARAM_Props, volumes, mute));
    if let Some(bytes) = serialize(props)
        && let Some(pod) = Pod::from_bytes(&bytes)
    {
        n.set_param(ParamType::Props, 0, pod);
    }
}

/// Run the connection until it ends; `Ok(true)` when the server went away (not asked to quit).
fn run(
    graph: &SharedGraph,
    notifier: &Notifier,
    rx: pw::channel::Receiver<Cmd>,
) -> Result<bool, pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    lock(graph).connected = true;
    notifier.touch(dirty::AVAILABLE);

    let lost = Rc::new(Cell::new(false));
    let _core_listener = core
        .add_listener_local()
        .error({
            let ml = main_loop.downgrade();
            let lost = lost.clone();
            move |id, _seq, res, message| {
                // an error on the core object itself: the connection is gone (EPIPE)
                if id == pw::core::PW_ID_CORE {
                    eprintln!("deskd: mixer: PipeWire connection lost ({res}: {message})");
                    lost.set(true);
                    if let Some(ml) = ml.upgrade() {
                        ml.quit();
                    }
                }
            }
        })
        .register();

    let objs = Rc::new(RefCell::new(Objects::default()));
    let _registry_listener = registry
        .add_listener_local()
        .global({
            let registry = registry.downgrade();
            let objs = Rc::downgrade(&objs);
            let graph = graph.clone();
            let notifier = notifier.clone();
            move |obj| {
                let (Some(registry), Some(objs)) = (registry.upgrade(), objs.upgrade()) else {
                    return;
                };
                let Some(props) = obj.props else {
                    return;
                };
                match obj.type_ {
                    ObjectType::Node => {
                        let flags = lock(&graph).add_node(obj.id, copy_props(props));
                        if flags == 0 && lock(&graph).node(obj.id).is_none() {
                            return; // not audio
                        }
                        notifier.touch(flags);
                        let Ok(node) = registry.bind::<Node, _>(obj) else {
                            return;
                        };
                        let listener = node_listener(&node, obj.id, &graph, &notifier, &objs);
                        node.subscribe_params(&[ParamType::Props]);
                        objs.borrow_mut().nodes.insert(obj.id, (node, listener));
                    }
                    ObjectType::Device if props.get("media.class") == Some("Audio/Device") => {
                        let Ok(device) = registry.bind::<Device, _>(obj) else {
                            return;
                        };
                        let id = obj.id;
                        let listener = device
                            .add_listener_local()
                            .param({
                                let graph = graph.clone();
                                let notifier = notifier.clone();
                                move |_seq, kind, _index, _next, param| {
                                    if kind == ParamType::Route
                                        && let Some(route) =
                                            deserialize(param).and_then(parse_route)
                                    {
                                        notifier.touch(lock(&graph).device_route(id, route));
                                    }
                                }
                            })
                            .register();
                        device.subscribe_params(&[ParamType::Route]);
                        objs.borrow_mut().devices.insert(id, (device, listener));
                    }
                    ObjectType::Link => {
                        let get = |k| props.get(k).and_then(|v| v.parse::<u32>().ok());
                        if let (Some(out), Some(input)) =
                            (get("link.output.node"), get("link.input.node"))
                        {
                            notifier.touch(lock(&graph).add_link(obj.id, out, input));
                        }
                    }
                    ObjectType::Metadata if props.get("metadata.name") == Some("default") => {
                        let Ok(m) = registry.bind::<Metadata, _>(obj) else {
                            return;
                        };
                        let listener = m
                            .add_listener_local()
                            .property({
                                let graph = graph.clone();
                                let notifier = notifier.clone();
                                move |subject, key, _type, value| {
                                    notifier.touch(lock(&graph).metadata(subject, key, value));
                                    0
                                }
                            })
                            .register();
                        lock(&graph).metadata_id = Some(obj.id);
                        objs.borrow_mut().metadata = Some((obj.id, m, listener));
                    }
                    _ => {}
                }
            }
        })
        .global_remove({
            let objs = Rc::downgrade(&objs);
            let graph = graph.clone();
            let notifier = notifier.clone();
            move |id| {
                notifier.touch(lock(&graph).remove(id));
                if let Some(objs) = objs.upgrade() {
                    let mut o = objs.borrow_mut();
                    o.nodes.remove(&id);
                    o.devices.remove(&id);
                    if o.metadata.as_ref().is_some_and(|(m, _, _)| *m == id) {
                        o.metadata = None;
                    }
                }
            }
        })
        .register();

    let _receiver = rx.attach(main_loop.loop_(), {
        let ml = main_loop.downgrade();
        let objs = objs.clone();
        move |cmd| match cmd {
            Cmd::Exec(ops) => {
                let o = objs.borrow();
                for op in ops {
                    exec(&o, op);
                }
            }
            Cmd::Quit => {
                if let Some(ml) = ml.upgrade() {
                    ml.quit();
                }
            }
        }
    });

    main_loop.run();
    Ok(lost.get())
}

fn node_listener(
    node: &Node,
    id: u32,
    graph: &SharedGraph,
    notifier: &Notifier,
    objs: &Rc<RefCell<Objects>>,
) -> NodeListener {
    let objs: Weak<RefCell<Objects>> = Rc::downgrade(objs);
    node.add_listener_local()
        .info({
            let graph = graph.clone();
            let notifier = notifier.clone();
            move |info| {
                let mask = info.change_mask();
                let mut g = lock(&graph);
                let mut flags = 0;
                if mask.contains(NodeChangeMask::PROPS)
                    && let Some(p) = info.props()
                {
                    flags |= g.add_node(id, copy_props(p));
                }
                if mask.contains(NodeChangeMask::STATE) {
                    let running = matches!(info.state(), NodeState::Running);
                    flags |= g.node_state(id, running, Instant::now());
                }
                notifier.touch(flags);
            }
        })
        .param({
            let graph = graph.clone();
            let notifier = notifier.clone();
            move |_seq, kind, _index, _next, param| {
                if kind != ParamType::Props {
                    return;
                }
                let Some(o) = deserialize(param) else {
                    return;
                };
                let (volumes, mute) = volume_and_mute(o.properties);
                if volumes.is_none() && mute.is_none() {
                    return;
                }
                let (flags, restore) = lock(&graph).node_params(id, volumes, mute);
                notifier.touch(flags);
                if !restore.is_empty()
                    && let Some(objs) = objs.upgrade()
                {
                    let o = objs.borrow();
                    for op in restore {
                        exec(&o, op);
                    }
                }
            }
        })
        .register()
}
