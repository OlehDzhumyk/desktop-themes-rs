// SPDX-License-Identifier: GPL-3.0-or-later
//! state.json: сумісний із версією 1 (python) — старі поля зберігаються, нові додаються.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::modules::Module;
use crate::paths::Ctx;

pub const STATE_VERSION: u32 = 2;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct FlatpakChange {
    pub file: String,
    pub section: String,
    pub key: String,
    pub old: Option<String>,
    pub new: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BrokenExtension {
    pub uuid: String,
    pub reason: String,
    pub since: String,
    /// Версія GNOME Shell на момент вимкнення.
    #[serde(default)]
    pub shell_version: String,
    /// Позначка metadata.json (mtime:version) — змінюється, коли розширення оновлюють.
    #[serde(default)]
    pub stamp: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub version: u32,
    /// Тема модуля «вигляд» (так поле поводилося у версії 1).
    #[serde(default)]
    pub current: Option<String>,
    /// Звідки взято кожен модуль.
    #[serde(default)]
    pub modules: BTreeMap<Module, String>,
    /// Об'єднання module_keys (для сумісності з версією 1).
    #[serde(default)]
    pub applied_keys: Vec<String>,
    #[serde(default)]
    pub module_keys: BTreeMap<Module, Vec<String>>,
    #[serde(default)]
    pub applied_extensions: Vec<String>,
    #[serde(default)]
    pub links: BTreeMap<String, String>,
    #[serde(default)]
    pub backups: BTreeMap<String, String>,
    #[serde(default)]
    pub created_dirs: Vec<String>,
    #[serde(default)]
    pub flatpak: Vec<FlatpakChange>,
    /// Тема, чий apps/hooks/apply виконано останнім (для revert).
    #[serde(default)]
    pub hooks_applied: Option<String>,
    /// Розширення, вимкнені «лікарем» або автовідкатом; повертаються, коли їх полагодять.
    #[serde(default)]
    pub broken_extensions: Vec<BrokenExtension>,
    #[serde(default)]
    pub gnome_version: Option<String>,
    #[serde(default)]
    pub last_doctor: Option<String>,
    /// Невідомі поля зберігаються як є.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl State {
    pub fn load(ctx: &Ctx) -> State {
        let st: State = std::fs::read_to_string(&ctx.state)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        st.migrate(ctx)
    }

    /// Версія 1 знала лише одну тему на все: вважаємо, що всі модулі взято з неї.
    fn migrate(mut self, ctx: &Ctx) -> State {
        if self.current.is_none() {
            self.current = std::fs::read_to_string(&ctx.current)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
        if self.version < 2 {
            if let Some(cur) = self.current.clone() {
                for m in Module::ALL {
                    self.modules.entry(m).or_insert_with(|| cur.clone());
                }
            }
            if self.module_keys.is_empty() && !self.applied_keys.is_empty() {
                self.module_keys
                    .insert(Module::Look, self.applied_keys.clone());
            }
            self.version = STATE_VERSION;
        }
        self
    }

    pub fn save(&mut self, ctx: &Ctx) -> Result<()> {
        self.version = STATE_VERSION;
        let mut all: Vec<String> = self.module_keys.values().flatten().cloned().collect();
        all.sort();
        all.dedup();
        self.applied_keys = all;
        self.current = self
            .modules
            .get(&Module::Look)
            .cloned()
            .or(self.current.take());
        let tmp = ctx.state.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)? + "\n")?;
        std::fs::rename(&tmp, &ctx.state)?;
        if let Some(c) = &self.current {
            std::fs::write(&ctx.current, format!("{c}\n"))?;
        }
        Ok(())
    }

    pub fn source(&self, m: Module) -> Option<&str> {
        self.modules.get(&m).map(String::as_str)
    }

    /// Тема, з якої взято всі модулі, або None, якщо їх змішано.
    pub fn uniform(&self) -> Option<&str> {
        let first = self.source(Module::Look)?;
        Module::ALL
            .iter()
            .all(|m| self.source(*m) == Some(first))
            .then_some(first)
    }

    pub fn is_broken(&self, uuid: &str) -> bool {
        self.broken_extensions.iter().any(|b| b.uuid == uuid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tests::{tmp_root, write};

    #[test]
    fn migrate_v1_and_keep_unknown_fields() {
        let root = tmp_root("state");
        let ctx = Ctx::new(root.join("home"), root.clone());
        write(
            &ctx.state,
            r#"{"current":"windows11","applied_keys":["/a/b","/c/d"],"applied_extensions":["x@y"],
            "links":{"/h/.config/gtk-4.0/gtk.css":"/t/gtk.css"},"backups":{},"created_dirs":["/h/.config/gtk-4.0"],
            "flatpak":[{"file":"/f/global","section":"Context","key":"filesystems","old":null,"new":"a;"}],
            "future_field":{"z":1}}"#,
        );
        let mut st = State::load(&ctx);
        assert_eq!(st.version, 2);
        assert_eq!(st.source(Module::Look), Some("windows11"));
        assert_eq!(st.source(Module::Apps), Some("windows11"));
        assert_eq!(st.uniform(), Some("windows11"));
        assert_eq!(st.module_keys[&Module::Look], vec!["/a/b", "/c/d"]);
        assert_eq!(st.flatpak[0].old, None);
        st.modules.insert(Module::Behavior, "macos".into());
        assert_eq!(st.uniform(), None);
        st.module_keys
            .insert(Module::Behavior, vec!["/e/f".into(), "/a/b".into()]);
        st.save(&ctx).unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&ctx.state).unwrap()).unwrap();
        assert_eq!(raw["current"], "windows11");
        assert_eq!(raw["modules"]["behavior"], "macos");
        assert_eq!(
            raw["applied_keys"],
            serde_json::json!(["/a/b", "/c/d", "/e/f"])
        );
        assert_eq!(raw["future_field"]["z"], 1);
        assert_eq!(
            std::fs::read_to_string(&ctx.current).unwrap(),
            "windows11\n"
        );
        // повторне завантаження v2 нічого не перезаписує
        let st2 = State::load(&ctx);
        assert_eq!(st2.source(Module::Behavior), Some("macos"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_state() {
        let root = tmp_root("state-empty");
        let ctx = Ctx::new(root.join("home"), root.clone());
        let st = State::load(&ctx);
        assert!(st.modules.is_empty());
        assert_eq!(st.uniform(), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
