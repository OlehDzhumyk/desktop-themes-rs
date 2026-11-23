// SPDX-License-Identifier: GPL-3.0-or-later
//! Tests of the panic policy (audit R-3): a panic in one handler must not end the daemon.
//!
//! The release profile unwinds (`panic = "unwind"` in the workspace `Cargo.toml`), because with
//! `abort` any panic in any thread (a D-Bus handler, the PipeWire thread) kills clipboard history
//! and mixer together. The zbus dispatcher runs on tokio, which catches a panic of one task.

#[cfg(test)]
mod tests {
    use zbus::interface;

    struct Boom;

    #[interface(name = "test.Boom")]
    impl Boom {
        async fn boom_async(&self) {
            panic!("test panic in an async handler");
        }
        fn boom_sync(&self) {
            panic!("test panic in a sync handler");
        }
        fn ping(&self) -> u32 {
            7
        }
    }

    /// The workspace must not go back to `panic = "abort"` for the release build.
    #[test]
    fn release_profile_unwinds() {
        let toml = include_str!("../../../Cargo.toml");
        let release = toml
            .split("[profile.release]")
            .nth(1)
            .expect("[profile.release] section");
        let release = release.split("\n[").next().unwrap_or(release);
        assert!(
            !release.contains("\"abort\""),
            "deskd must unwind on panic; see the comment in Cargo.toml"
        );
    }

    /// A panicking handler (sync or async) leaves the connection and the other methods working.
    #[test]
    fn a_panicking_handler_does_not_end_the_service() {
        zbus::block_on(async {
            let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
            let server = zbus::connection::Builder::unix_stream(a)
                .server(zbus::Guid::generate())
                .p2p()
                .serve_at("/b", Boom)
                .build();
            let client = zbus::connection::Builder::unix_stream(b).p2p().build();
            let (_server, client) = futures_util::future::try_join(server, client)
                .await
                .unwrap();
            for method in ["BoomAsync", "BoomSync", "BoomAsync", "BoomSync"] {
                let msg = zbus::message::Message::method_call("/b", method)
                    .interface("test.Boom")
                    .build(&())
                    .unwrap();
                // no reply will come; only the service must survive
                client.send(&msg).await.unwrap();
            }
            for _ in 0..3 {
                let reply = client
                    .call_method(None::<&str>, "/b", Some("test.Boom"), "Ping", &())
                    .await
                    .unwrap();
                assert_eq!(reply.body().deserialize::<u32>().unwrap(), 7);
            }
        });
    }
}
