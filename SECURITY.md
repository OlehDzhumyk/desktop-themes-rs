# Security policy

## Supported versions

Only the latest release and the `main` branch receive security fixes.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub:
<https://github.com/OlehDzhumyk/desktop-themes-rs/security/advisories/new>
("Report a vulnerability" on the repository's Security tab).

Please include what you found and its impact, the affected version or commit, and the steps to
reproduce. You can expect an acknowledgement within a few days. Fixes are prepared in a private
advisory, released, and credited to you unless you prefer to stay anonymous.

## What is in scope

desktop-themes-rs runs entirely as the logged-in user. Things we treat as security issues:

* **deskd and the clipboard history**: reading or writing history without the user's consent,
  recording content that should be excluded (password-manager hints, excluded apps, private mode),
  weak file permissions on `~/.local/share/deskd/` or `$XDG_RUNTIME_DIR/deskd/`, other local users
  or sandboxed apps reaching the D-Bus interface in a way the design does not intend.
* **The theme manager**: a theme (including a downloaded one) that can run code other than the
  documented `apps/hooks/apply|revert` hooks, writes outside the documented locations, or
  escapes path expansion; unsafe handling of `state.json`, backups or the lock file.
* **Scripts**: `scripts/` and `data/install.sh` downloading anything without the pinned revision
  or checksum, or writing outside the user's home.
* **Release artifacts**: tampering with the release workflow, checksums or provenance.

Two guards limit what a theme can do besides its hooks ([theme contract](docs/theme-contract.md)):
Flatpak overrides in `flatpak.conf` are limited to read-only GTK, theme and icon folders and
`GTK_THEME` (anything else is refused with a warning), and dconf keys that run commands (custom
shortcuts, terminal `exec`, autostart) are listed in `apply --dry-run` and need a yes (`--yes` or the
window's dialog) the first time.

Theme hooks and the scripts you run yourself execute with your privileges by design: reviewing a
theme from an untrusted source before applying it is the user's responsibility, and the
documentation says so. Third-party extensions and themes that bootstrap downloads are separate
projects; report problems in them upstream.

## Verifying releases

Release archives come with `SHA256SUMS` and a GitHub build provenance attestation:

```sh
sha256sum --check SHA256SUMS
gh attestation verify desktop-themes-rs-VERSION-x86_64.tar.gz --repo OlehDzhumyk/desktop-themes-rs
```
