// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus-інтерфейси deskd: `io.github.OlehDzhumyk.Desk` і `io.github.OlehDzhumyk.Desk.Clipboard`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use desk_proto::ClipItem;
use zbus::object_server::SignalEmitter;

use crate::clipboard::{Engine, Pushed};

pub type Shared = Arc<Mutex<Engine>>;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn lock(e: &Shared) -> MutexGuard<'_, Engine> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

fn save(eng: &Engine) {
    if let Err(e) = eng.save() {
        eprintln!("deskd: cannot save the clipboard history: {e}");
    }
}

fn not_found(id: u64) -> zbus::fdo::Error {
    zbus::fdo::Error::InvalidArgs(format!("no item {id}"))
}

pub struct Desk {
    pub features: Vec<String>,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk")]
impl Desk {
    #[zbus(property)]
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    #[zbus(property)]
    fn features(&self) -> Vec<String> {
        self.features.clone()
    }
}

pub struct ClipboardIface {
    pub eng: Shared,
    /// Записати приватний режим у GSettings (у тестах — нічого).
    pub set_private: fn(bool),
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Clipboard")]
impl ClipboardIface {
    async fn push(
        &self,
        mime: String,
        data: Vec<u8>,
        offered: Vec<String>,
        source_app: String,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> u64 {
        let r = {
            let mut eng = lock(&self.eng);
            let r = eng.push(&mime, &data, &offered, &source_app, now());
            if !matches!(r, Pushed::Skipped(_)) {
                save(&eng);
            }
            r
        };
        match r {
            Pushed::Added(id) | Pushed::Bumped(id) => {
                let _ = Self::changed(&em).await;
                let _ = self.count_changed(&em).await;
                id
            }
            Pushed::Skipped(_) => 0,
        }
    }

    fn list(&self, query: String, limit: u32) -> Vec<ClipItem> {
        lock(&self.eng).list(&query, limit)
    }

    #[zbus(out_args("mime", "data"))]
    fn get(&self, id: u64) -> zbus::fdo::Result<(String, Vec<u8>)> {
        lock(&self.eng).get(id).ok_or_else(|| not_found(id))
    }

    #[zbus(out_args("mime", "data"))]
    async fn activate(
        &self,
        id: u64,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<(String, Vec<u8>)> {
        let r = {
            let mut eng = lock(&self.eng);
            let r = eng.activate(id, now());
            save(&eng);
            r
        };
        let r = r.ok_or_else(|| not_found(id))?;
        let _ = Self::changed(&em).await;
        Ok(r)
    }

    async fn pin(
        &self,
        id: u64,
        pinned: bool,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let found = {
            let mut eng = lock(&self.eng);
            let found = eng.find(id).is_some();
            if eng.pin(id, pinned) {
                save(&eng);
            }
            found
        };
        if !found {
            return Err(not_found(id));
        }
        let _ = Self::changed(&em).await;
        Ok(())
    }

    async fn delete(&self, id: u64, #[zbus(signal_emitter)] em: SignalEmitter<'_>) {
        let changed = {
            let mut eng = lock(&self.eng);
            let c = eng.delete(id);
            if c {
                save(&eng);
            }
            c
        };
        if changed {
            let _ = Self::changed(&em).await;
            let _ = self.count_changed(&em).await;
        }
    }

    async fn clear(&self, keep_pinned: bool, #[zbus(signal_emitter)] em: SignalEmitter<'_>) {
        {
            let mut eng = lock(&self.eng);
            eng.clear(keep_pinned);
            save(&eng);
        }
        let _ = Self::changed(&em).await;
        let _ = self.count_changed(&em).await;
    }

    #[zbus(property)]
    fn private(&self) -> bool {
        lock(&self.eng).cfg.private
    }

    #[zbus(property)]
    fn set_private(&mut self, value: bool) {
        lock(&self.eng).cfg.private = value;
        (self.set_private)(value);
    }

    #[zbus(property)]
    fn enabled(&self) -> bool {
        lock(&self.eng).cfg.enabled
    }

    #[zbus(property)]
    fn count(&self) -> u32 {
        lock(&self.eng).len() as u32
    }

    #[zbus(signal)]
    pub async fn changed(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use desk_proto::{CLIPBOARD_XML, DESK_XML, xml_members};
    use zbus::object_server::Interface;

    use super::*;

    /// Члени, які генерує `#[zbus::interface]`, збігаються з data/dbus/*.xml (його читає міст).
    #[test]
    fn introspection_matches_xml() {
        let mut xml = String::new();
        let eng = Engine::new(Default::default(), "/nonexistent".into(), "/nonexistent".into());
        let iface = ClipboardIface {
            eng: Arc::new(Mutex::new(eng)),
            set_private: |_| {},
        };
        iface.introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(CLIPBOARD_XML), "{xml}");
        assert!(xml.contains("a(tssssbxt)"));
        let mut xml = String::new();
        Desk { features: vec![] }.introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(DESK_XML));
    }
}
