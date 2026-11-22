# Phone integration

## Summary

Use the phone with the desktop on the home network: see the phone's notifications and battery on the desktop, reply to messages, send files and links
both ways, share the clipboard (opt-in), ring a lost phone, control media and the pointer. Everything stays on the local network; there is no cloud account.

## Status

**Planned** (stage 5). Origin badge: *In Windows 11 and macOS* (Phone Link; Continuity/iPhone Mirroring), built *on GSConnect (GPL-2.0-or-later)*,
the GNOME Shell implementation of the KDE Connect protocol. We integrate it (principle 5), not rewrite it; licence compatibility with
GPL-3.0-or-later is checked when the extension is bundled or ported.

## Reference behaviour

* **Windows 11 (Phone Link):** pairs an Android phone through a QR code; shows notifications, messages (send and receive), calls, photos and
  recent apps on the PC; needs a Microsoft account and the cloud for some parts.
* **macOS (Continuity):** iPhone notifications and calls on the Mac, SMS relay, universal clipboard and Handoff, AirDrop, iPhone Mirroring; needs
  the same Apple account; uses Bluetooth and Wi-Fi directly.

## Our behaviour

### Pairing

1. Hub page «Телефон» lists devices found on the LAN (KDE Connect protocol, UDP 1716 discovery, TCP 1714-1764). The phone runs the **KDE Connect**
   app (Android; iOS support is limited).
2. «Підключити» sends a pairing request; both sides show the same **verification key**; the user accepts on both. Pairing stores the peer's TLS
   certificate (trust on first use after the explicit confirmation). A QR-code route is optional (open question 2).
3. Unpair from either side removes the certificate. A device unseen for 30 days is marked «Давно не бачили».

### Plugins (each switchable per device)

| Plugin | Default | Behaviour |
|---|---|---|
| Notifications from the phone | on | shown as desktop notifications in the shell with the app name and a «Phone» badge; dismissing on one side dismisses on the other; **content never logged or stored by us**; quiet while [focus](focus.md) mode blocks banners |
| Reply to messages | on | inline reply from a notification action; a small «Повідомлення» window lists conversations (no history stored by us beyond what GSConnect keeps) |
| Battery | on | level and charging state in the hub and in a quick-settings sub-menu of the «Телефон» tile; low-battery notice at <= 15% (setting) |
| Send / receive files and links | on | share target «Надіслати на телефон» in Files and the share menu; received files go to `~/Downloads/Phone/` with a notification; `Esc`-able confirmation for files > 100 MB |
| Clipboard sync | **off** | when on: a copy on one side offers the other; to the phone only when the copy came from the history panel by an explicit «Надіслати на телефон» action unless `auto-send` is on; from the phone: appended to the history as normal (private mode applies) |
| Find my phone | on | «Подзвонити» rings the phone at full volume until dismissed |
| Media control | on | the phone's player shows in the media controls |
| Remote input | off | phone as touchpad/keyboard; needs explicit enabling per device |
| Run commands, SMS send, contact sync | off | contacts are read only to show names in notifications; not stored by us |

### Skins

* `windows11`: a "Phone Link"-style flyout (tile in quick settings → «Телефон»): device name, battery, recent notifications, buttons Messages / Send
  file / Find. 
* `macos`: Control Center-style module «iPhone»-like (we name it after the device, e.g. «Pixel»): battery, Send file, Find, Notifications toggle.
* `default`: the GSConnect indicator menu, themed; sub-menu in quick settings.
The hub page is shared; layout follows the skin.

### Keyboard, focus, animation

All device actions are menu items reachable with `Tab`/arrows/`Enter`; notification actions are keyboard-operable in the shell's notification
UI; no animation of ours beyond the shell's.

### Empty and error states

No devices: «Телефонів поки немає. Встановіть KDE Connect на телефон і підключіться до тієї самої мережі.» with a «Показати ключі брандмауера» help; device
offline: greyed with «Не в мережі»; port blocked: the hub explains the firewall ports (**Silverblue's firewalld may block 1714-1764**) and gives
the `firewall-cmd` command for the user to run; we do not change the firewall.

## Settings (proposed, `io.github.OlehDzhumyk.Desk.Phone`, plus GSConnect's own schema which we do not duplicate)

`enabled` (false until the user adds a phone), `notify-battery-low` (uint 0..50, 15), `download-folder` (string, `~/Downloads/Phone`), per-device
plugin switches (GSConnect's), `clipboard-auto-send` (bool, false).

## Privacy & security

LAN only, TLS with pinned peer certificates; **no cloud, no account, no telemetry**. Notifications and message text are shown but not written to disk by
us; clipboard sync is off by default and honours private mode and password-manager hints (hinted content is never offered to a phone). Received files
are never executed or auto-opened. Pairing requires confirmation on both devices; an unpaired device cannot see anything. Discovery broadcasts the device name
(user-editable): documented. Untrusted-network warning: on networks marked public the hub disables discovery unless the user opts in.

## Edge cases

* Phone and PC on different subnets or client isolation Wi-Fi: not found; message explains.
* Two PCs paired to one phone: works (per-device pairing).
* PC suspended: the phone sees it offline; on resume re-announces within 10 s.
* VPN active on the PC: discovery may fail; manual IP address entry provided.
* Notification storms: coalesced per app (max 5 per 10 s), the rest in the list.
* Very large file transfers: progress notification, cancel button, resume not promised.
* Duplicate notifications (phone also bridged elsewhere): dedup by id.
* Locale: names and texts in the UI language; message bodies as sent.

## Accessibility

Device list, plugin switches and the flyout are keyboard and screen-reader operable; battery is stated as text («87%, заряджається»); the find-my-phone
ring has a visible stop; notifications keep the shell's accessibility.

## Performance

Discovery is on-demand (UDP listen while the hub page, tile or paired devices exist); GSConnect's own idle cost counts toward the shell, tracked
by the bridge-memory budget (<= 10 MB for our part); deskd holds only settings: no resident growth; no idle wakeups from us.

## Acceptance criteria

* AC-PHONE-01: Given a phone on the same LAN with KDE Connect, When the hub page is opened, Then the phone is listed within 5 s.
* AC-PHONE-02: Given pairing, Then both sides show the same verification key and the device is trusted only after both accept.
* AC-PHONE-03: Given a phone notification, Then a desktop notification appears with the app name; dismissing it dismisses it on the phone.
* AC-PHONE-04: Given clipboard sync off, When text is copied on the PC, Then nothing is sent to the phone.
* AC-PHONE-05: Given clipboard sync on and a copy with `x-kde-passwordManagerHint`, Then it is not offered.
* AC-PHONE-06: Given a file shared to the phone, Then it arrives with a progress notification; cancel stops it.
* AC-PHONE-07: Given a focus mode blocking banners, Then phone notifications obey it.
* AC-PHONE-08: Given unpair, Then the certificate is removed and the device reappears as new.
* AC-PHONE-09: Given a blocked port, Then the hub shows the firewall help and no firewall change is attempted.
* AC-PHONE-10: Given no network access at all, Then the app does not hang; the page shows no devices.

## Open questions

1. Is bundling GSConnect (as `extensions.txt` for all themes, like the bridge) acceptable, or an opt-in module installed on first use?
2. QR-code pairing (Phone Link style) needs protocol support: upstream GSConnect or skip?
3. iOS: only partial KDE Connect support exists; state this plainly in the UI or hide the page without an Android phone?
4. A shared desktop-wide inbox for phone and PC notifications (a single history) is out of scope: confirm.
