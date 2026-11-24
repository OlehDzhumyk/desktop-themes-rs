# 0008. deskd serves content only to GNOME Shell and its own binaries

* Status: accepted
* Date: 2026-11-23

## Context

deskd lives on the session bus, which every process of the user can call. Before this decision
any client could read the whole clipboard history (`List`, `Get`, `Activate`, text and images),
including sandboxed apps with session-bus access (Flatpak `--socket=session-bus` or a
`--talk-name` for our name). On Wayland the clipboard is only readable by the focused client;
deskd removed that boundary. The bus name was requested with `ALLOW_REPLACEMENT`, so any process
could take it (`RequestName` with `REPLACE_EXISTING`), receive every new clipboard content
(`Push`) and choose what the bridge pastes with a synthesized `Shift+Insert`, terminals
included. Stage 3 (screenshots, OCR) would expose screen content the same way.

Options:

1. **Check the sender of each call** (bus credentials plus `/proc`).
2. **Move content methods to a private peer-to-peer socket** handed to the bridge only.

Option 2 still has to decide whom to hand the socket to, through the same bus, so it needs the
check of option 1 anyway, plus a second transport, socket lifetime across shell and deskd
restarts, and fd passing in GJS. Option 1 is one function and works with dbus-broker and
dbus-daemon alike.

## Decision

deskd checks the caller of every method that returns or changes user content
(`crates/desk-proto/src/trust.rs`, shared with the theme service below). A caller is trusted when
all of these hold:

1. same user (`UnixUserID` from `GetConnectionCredentials`);
2. same root directory as deskd (`/proc/<pid>/root`, compared by device and inode). Flatpak,
   Snap and other bwrap sandboxes give the app its own root, and a Flatpak app without the bus
   socket reaches the bus through `xdg-dbus-proxy`, which is sandboxed too. This rule is what
   stops sandboxed apps; it holds even when such an app owns `org.gnome.Shell`;
3. its executable (`/proc/<pid>/exe`, by device and inode, so links and renames do not matter) is
   the service itself or the `deskd`, `themes` or `desk-shell` binary next to it (`desk-shell`:
   [ADR 0012](0012-wayland-layer-shell-desk-shell.md)), **or** it is the current owner of
   `org.gnome.Shell`. The owner is asked with `GetNameOwner` on each call that rules 1 and 2 pass
   and the executable rule does not, so a restarted shell is followed, a process that lost the
   name loses the trust, and our own binaries (`desk-shell` on compositors without GNOME Shell)
   never pay for that round trip. The bus proxy is built once per connection.

When the bus passes a pidfd (`ProcessFD`), the pid must still belong to that process after
`/proc` was read, so a reused pid cannot lend another process's identity.

Gated: Clipboard `Push`, `List`, `Get`, `Activate`, `Pin`, `Delete`, `Clear`, setting `Private`;
every Mixer method. Media titles and app names are mildly sensitive, and `SetMute`/`SetDefault`
could unmute or switch the microphone, so the whole Mixer method set is gated. Open: `Version`,
`Features`, `Count`, `Enabled`, `Private` (read), the Mixer properties (ids and availability), and
the `Changed` signals, which carry no content. Refusals return
`org.freedesktop.DBus.Error.AccessDenied` and are logged once per sender (at most 64 senders).

The theme service (`themes --dbus-service`, `io.github.OlehDzhumyk.Themes`) applies the same
check: `Apply`, `ApplyConfirmed` and `PendingCommands` (it reads the user's settings and the
approved commands) and `Doctor` (which can fix and notify) are gated; `List`, `Current`,
`Experimental` and the `Changed` signal carry only theme metadata and stay open. Before this, any
process with session-bus access, a Flatpak app included, could apply a theme with
`ApplyConfirmed` and so approve the theme's command keys (S-5) without the user. Its bus name is
owned with `DO_NOT_QUEUE` only: zbus's default flags include `ALLOW_REPLACEMENT`, which let any
process take the name and receive the bridge's applies.

The bus name is requested without `ALLOW_REPLACEMENT`. Only the development mode `--replace`
takes the name from a `--replace` deskd and leaves it replaceable again. The installed deskd
(systemd unit, D-Bus activation) is never replaceable; restarts go through systemd.

The bridge checks the other direction (`bridge/src/trust.ts`): the owner of
`io.github.OlehDzhumyk.Desk` must be the same user with the same root, and its executable the
file that deskd's D-Bus activation file names (user and system data dirs) or
`~/.local/bin/deskd`. Clipboard methods are sent to that verified unique name, never to the
well-known name, so a later owner never receives content. A refused owner is logged once and
nothing is pushed to or pasted from it. The Mixer client is not checked: a fake owner can only
show wrong mixer rows, and the bridge sends it no content.

## Consequences

* Good: sandboxed apps and other users get nothing; a takeover of the name is impossible for the
  installed deskd and harmless for the bridge; the same rule covers the stage 3 engines.
* Good: one extra bus round trip and a few `/proc` reads per gated call (user actions and copies).
* Bad / cost: `busctl`/`gdbus` cannot read the history or apply a theme through the service any
  more (`themes apply` on the command line is unaffected); debugging goes through the shell
  (Looking Glass) or the e2e hooks. Tests that stand in for the shell own `org.gnome.Shell` on
  their private bus.
* Bad / cost: a development build of deskd that is not installed (`target/debug/deskd`) is
  refused by the bridge. Install it (`data/install.sh --deskd target/debug/deskd`) or use the
  e2e harness, whose activation file points at the build.
* Not a boundary: unsandboxed processes of the same user. They can read
  `~/.local/share/deskd` and `$XDG_RUNTIME_DIR/deskd` directly, or attach a debugger to the shell,
  so the executable rule only stops casual access; see SECURITY.md.

## Alternatives considered

* Private peer-to-peer socket for the bridge: needs the same caller check to hand out the socket,
  plus a second transport and its lifetime handling (see Context).
* Trust by bus name only (`org.gnome.Shell`): a Flatpak app with the bus socket could own the name
  while the shell is not running; the root rule closes that.
* Trust by executable path string: a sandbox can show any path inside its own root; device and
  inode of the real file cannot be faked.
* A polkit or portal-style consent prompt: wrong tool for a same-session daemon; nothing to ask the
  user that the rules above do not settle.
