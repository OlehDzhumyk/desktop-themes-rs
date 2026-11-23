// SPDX-License-Identifier: GPL-3.0-or-later
//! D-Bus interface `io.github.OlehDzhumyk.Desk.Emoji` (data/dbus/io.github.OlehDzhumyk.Desk.Emoji.xml).

use std::sync::{Arc, Mutex, MutexGuard};

use desk_proto::emoji::EmojiItem;
use zbus::message::Header;

use super::EmojiEngine;
use crate::trust::Gate;

pub type SharedEmoji = Arc<Mutex<EmojiEngine>>;

pub fn lock(m: &SharedEmoji) -> MutexGuard<'_, EmojiEngine> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct EmojiIface {
    pub engine: SharedEmoji,
    /// Only GNOME Shell and desktop-themes-rs binaries may query or update recents (ADR 0008).
    pub gate: Gate,
}

#[zbus::interface(name = "io.github.OlehDzhumyk.Desk.Emoji")]
impl EmojiIface {
    async fn search(
        &self,
        query: String,
        lang: String,
        limit: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<EmojiItem>> {
        self.gate.check(conn, Some(&hdr)).await?;
        let mut eng = lock(&self.engine);
        if !lang.is_empty() {
            eng.set_lang(lang.clone());
        }
        Ok(eng.search(&query, &lang, limit))
    }

    async fn category(
        &self,
        category: String,
        limit: u32,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<EmojiItem>> {
        self.gate.check(conn, Some(&hdr)).await?;
        Ok(lock(&self.engine).category(&category, limit))
    }

    async fn recents(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<Vec<EmojiItem>> {
        self.gate.check(conn, Some(&hdr)).await?;
        Ok(lock(&self.engine).recents())
    }

    async fn pick(
        &self,
        id: String,
        tone: String,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<String> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine)
            .pick(&id, &tone)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("unknown emoji id: {id}")))
    }

    async fn clear_recents(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        self.gate.check(conn, Some(&hdr)).await?;
        lock(&self.engine).clear_recents();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    use desk_proto::emoji::EmojiProxy;
    use desk_proto::{EMOJI_PATH, EMOJI_XML, xml_members};
    use zbus::object_server::Interface;

    use super::super::data::{apply_skin_tone, dataset};
    use super::super::recents::{MAX_RECENTS, Recents};
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deskd-emoji-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("emoji.json")
    }

    #[test]
    fn emoji_introspection_matches_xml() {
        let p = temp_path("introspect");
        let eng = Arc::new(Mutex::new(EmojiEngine::new(
            p.clone(),
            false,
            "none".into(),
        )));
        let mut xml = String::new();
        EmojiIface {
            engine: eng,
            gate: Gate::open(),
        }
        .introspect_to_writer(&mut xml, 0);
        assert_eq!(xml_members(&xml), xml_members(EMOJI_XML), "{xml}");
        assert!(xml.contains("a(sssssb)"), "{xml}");
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn dataset_has_rich_coverage() {
        let all = dataset();
        assert!(
            all.len() >= 220,
            "expected >= 220 entries, got {}",
            all.len()
        );
        for cat in [
            "smileys",
            "people",
            "animals",
            "food",
            "travel",
            "activities",
            "objects",
            "symbols",
            "flags",
            "arrows",
            "math",
            "currency",
            "punctuation",
            "text-faces",
        ] {
            assert!(
                all.iter().any(|e| e.category == cat),
                "missing category {cat}"
            );
        }
    }

    #[test]
    fn uk_and_en_search_ranking() {
        let p = temp_path("search");
        let mut eng = EmojiEngine::new(p.clone(), true, "none".into());

        // AC-EMO-02: Ukrainian query "усмішка" matches smileys
        let uk_res = eng.search("усмішка", "uk", 20);
        assert!(!uk_res.is_empty());
        assert!(
            uk_res
                .iter()
                .any(|i| i.glyph == "🙂" || i.glyph == "😀" || i.glyph == "😄"),
            "uk search results: {uk_res:?}"
        );

        // AC-EMO-03: English query "smile" matches smileys
        let en_res = eng.search("smile", "en", 20);
        assert!(!en_res.is_empty());
        assert!(
            en_res
                .iter()
                .any(|i| i.glyph == "🙂" || i.glyph == "😀" || i.glyph == "😄"),
            "en search results: {en_res:?}"
        );

        // Apostrophe tolerance: "румяна" matches "рум'яна усмішка"
        let apos = eng.search("румяна", "uk", 10);
        assert!(apos.iter().any(|i| i.id == "blush"), "{apos:?}");

        // Recents tie-break: picking "grinning" promotes it above non-recent ties
        eng.pick("slightly_smiling", "none");
        let after = eng.search("smile", "en", 20);
        assert!(after.iter().any(|i| i.id == "slightly_smiling"));

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn skin_tone_modifier_application() {
        // AC-EMO-05: wave + medium -> 👋🏽
        assert_eq!(apply_skin_tone("👋", "medium", true), "👋🏽");
        assert_eq!(apply_skin_tone("👋", "light", true), "👋🏻");
        assert_eq!(apply_skin_tone("👋", "dark", true), "👋🏿");
        assert_eq!(apply_skin_tone("👋", "none", true), "👋");
        // Variation selector stripped before modifier
        assert_eq!(apply_skin_tone("✌️", "medium", true), "✌🏽");
        // ZWJ sequence keeps tail
        assert_eq!(apply_skin_tone("🧑‍💻", "medium", true), "🧑🏽‍💻");
        // Non-toneable emoji stays unchanged
        assert_eq!(apply_skin_tone("😀", "medium", false), "😀");
    }

    #[test]
    fn recents_persistence_and_clear() {
        let p = temp_path("recents");
        let mut r = Recents::new(p.clone(), true);
        for i in 0..(MAX_RECENTS + 5) {
            r.record(&format!("id_{i}"));
        }
        assert_eq!(r.ids().len(), MAX_RECENTS);
        assert_eq!(r.ids()[0], format!("id_{}", MAX_RECENTS + 4));

        // Deduplication moves existing item to front
        r.record("id_10");
        assert_eq!(r.ids()[0], "id_10");
        assert_eq!(r.ids().len(), MAX_RECENTS);

        // File permissions must be 0600
        let mode = std::fs::metadata(r.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // Reload from disk
        let mut r2 = Recents::new(p.clone(), true);
        r2.load();
        assert_eq!(r2.ids()[0], "id_10");
        assert_eq!(r2.ids().len(), MAX_RECENTS);

        // Turning remember off clears memory and deletes file
        r2.set_remember(false);
        assert!(r2.ids().is_empty());
        assert!(!p.exists());

        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn api_over_a_private_connection() {
        let p = temp_path("p2p");
        zbus::block_on(async {
            let eng = Arc::new(Mutex::new(EmojiEngine::new(
                p.clone(),
                true,
                "medium".into(),
            )));
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let guid = zbus::Guid::generate();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(guid)
                .p2p()
                .serve_at(
                    EMOJI_PATH,
                    EmojiIface {
                        engine: eng.clone(),
                        gate: Gate::open(),
                    },
                )
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (_server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();

            let proxy = EmojiProxy::builder(&client)
                .destination("io.github.OlehDzhumyk.Desk")
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .unwrap();

            let found = proxy.search("wave", "en", 10).await.unwrap();
            assert!(found.iter().any(|i| i.id == "wave" && i.supports_tone));

            let arrows = proxy.category("arrows", 5).await.unwrap();
            assert_eq!(arrows.len(), 5);

            // Empty tone falls back to configured default_tone ("medium")
            let glyph = proxy.pick("wave", "").await.unwrap();
            assert_eq!(glyph, "👋🏽");

            let recents = proxy.recents().await.unwrap();
            assert_eq!(recents.len(), 1);
            assert_eq!(recents[0].id, "wave");

            // Unknown id returns InvalidArgs
            let err = proxy.pick("nonexistent_id", "none").await.unwrap_err();
            assert!(err.to_string().contains("InvalidArgs"), "{err}");

            proxy.clear_recents().await.unwrap();
            assert!(proxy.recents().await.unwrap().is_empty());
        });
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
