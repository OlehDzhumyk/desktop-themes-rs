// SPDX-License-Identifier: GPL-3.0-or-later
// The "accent from wallpaper" control of the quick settings «Theme» menu, per skin
// (docs/spec/features/theme-from-wallpaper.md, "Skins"):
// * macos: an accent row of coloured dots (the GNOME named accents) plus a rainbow
//   «From wallpaper» dot, like macOS's Multicolour;
// * windows11: an «Accent colour» drop-down with «Automatic» and «Manual»;
// * default: the «Accent from wallpaper» switch.
// deskd does the work: the picker only writes `accent-from-wallpaper` and, for a named dot,
// org.gnome.desktop.interface `accent-color`.
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import St from 'gi://St';

import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {_} from './i18n.js';

/** io.github.OlehDzhumyk.Desk.Appearance: deskd follows the wallpaper while it is true. */
export const ACCENT_KEY = 'accent-from-wallpaper';
const INTERFACE_SCHEMA = 'org.gnome.desktop.interface';
const ACCENT_COLOR = 'accent-color';

/** GNOME's named accents (`accent-color` without `default`), in GNOME Settings' order. */
export const ACCENTS: [id: string, label: () => string][] = [
    ['blue', () => _('Blue')],
    ['teal', () => _('Teal')],
    ['green', () => _('Green')],
    ['yellow', () => _('Yellow')],
    ['orange', () => _('Orange')],
    ['red', () => _('Red')],
    ['pink', () => _('Pink')],
    ['purple', () => _('Purple')],
    ['slate', () => _('Slate')],
];

export type Skin = 'default' | 'macos' | 'windows11';

/** Which dot is selected: the wallpaper while it is followed, else the accent (`default` is blue). */
export function selectedDot(fromWallpaper: boolean, accent: string): string {
    if (fromWallpaper)
        return 'wallpaper';
    return accent === 'default' || accent === '' ? 'blue' : accent;
}

/** `org.gnome.desktop.interface` when it has `accent-color` (GNOME 47+), else null. */
function interfaceSettings(): Gio.Settings | null {
    const schema = Gio.SettingsSchemaSource.get_default()?.lookup(INTERFACE_SCHEMA, true);
    return schema?.has_key(ACCENT_COLOR) ? new Gio.Settings({schema_id: INTERFACE_SCHEMA}) : null;
}

/**
 * Fills `section` with the skin's control. Returns the function that disconnects its signal
 * handlers (call it before the section is emptied or destroyed).
 */
export function buildAccentPicker(section: PopupMenu.PopupMenuSection, skin: Skin,
    appearance: Gio.Settings): () => void {
    const iface = skin === 'macos' ? interfaceSettings() : null;
    if (skin === 'macos' && iface)
        return dotGrid(section, appearance, iface);
    if (skin === 'windows11')
        return dropDown(section, appearance);
    return plainSwitch(section, appearance);
}

function plainSwitch(section: PopupMenu.PopupMenuSection, appearance: Gio.Settings): () => void {
    const sw = new PopupMenu.PopupSwitchMenuItem(_('Accent from wallpaper'), appearance.get_boolean(ACCENT_KEY));
    sw.connect('toggled', (_i: unknown, on: boolean) => {
        appearance.set_boolean(ACCENT_KEY, on);
    });
    const id = appearance.connect(`changed::${ACCENT_KEY}`, () => {
        sw.setToggleState(appearance.get_boolean(ACCENT_KEY));
    });
    section.addMenuItem(sw);
    return () => { appearance.disconnect(id); };
}

/** macOS: «Accent colour» label and a row of dots; arrow keys and Tab move between the dots. */
function dotGrid(section: PopupMenu.PopupMenuSection, appearance: Gio.Settings,
    iface: Gio.Settings): () => void {
    const title = new PopupMenu.PopupMenuItem(_('Accent colour'), {reactive: false, can_focus: false});
    title.add_style_class_name('desk-accent-title');
    section.addMenuItem(title);
    const item = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false, style_class: 'desk-accent-row'});
    const box = new St.BoxLayout({style_class: 'desk-accent-dots', x_expand: true});
    box.accessible_role = Atk.Role.PANEL;
    box.accessible_name = _('Accent colour');
    item.add_child(box);
    const dots = new Map<string, St.Button>();
    const sync = () => {
        const sel = selectedDot(appearance.get_boolean(ACCENT_KEY), iface.get_string(ACCENT_COLOR));
        for (const [id, b] of dots) {
            b.checked = id === sel;
            if (id === sel)
                b.add_accessible_state(Atk.StateType.CHECKED);
            else
                b.remove_accessible_state(Atk.StateType.CHECKED);
        }
    };
    const add = (id: string, label: string) => {
        const b = new St.Button({
            style_class: `desk-accent-dot desk-accent-dot-${id}`,
            can_focus: true, reactive: true, toggle_mode: true,
            y_align: Clutter.ActorAlign.CENTER,
            accessible_name: label,
        });
        b.accessible_role = Atk.Role.RADIO_BUTTON;
        b.connect('clicked', () => {
            if (id === 'wallpaper') {
                appearance.set_boolean(ACCENT_KEY, true);
            } else {
                // the accent first, then the switch: deskd sees our accent as the one to keep
                iface.set_string(ACCENT_COLOR, id);
                appearance.set_boolean(ACCENT_KEY, false);
            }
            sync();
        });
        dots.set(id, b);
        box.add_child(b);
    };
    for (const [id, label] of ACCENTS)
        add(id, label());
    add('wallpaper', _('From wallpaper'));
    sync();
    section.addMenuItem(item);
    const ids: [Gio.Settings, number][] = [
        [appearance, appearance.connect(`changed::${ACCENT_KEY}`, sync)],
        [iface, iface.connect(`changed::${ACCENT_COLOR}`, sync)],
    ];
    return () => { ids.forEach(([s, id]) => { s.disconnect(id); }); };
}

/** Windows 11: «Accent colour» drop-down with «Automatic» (from the wallpaper) and «Manual». */
function dropDown(section: PopupMenu.PopupMenuSection, appearance: Gio.Settings): () => void {
    const sub = new PopupMenu.PopupSubMenuMenuItem(_('Accent colour'), false);
    const auto = new PopupMenu.PopupMenuItem(_('Automatic'));
    const manual = new PopupMenu.PopupMenuItem(_('Manual'));
    auto.connect('activate', () => { appearance.set_boolean(ACCENT_KEY, true); });
    manual.connect('activate', () => { appearance.set_boolean(ACCENT_KEY, false); });
    sub.menu.addMenuItem(auto);
    sub.menu.addMenuItem(manual);
    const sync = () => {
        const on = appearance.get_boolean(ACCENT_KEY);
        const state = on ? _('Automatic') : _('Manual');
        sub.label.text = `${_('Accent colour')}: ${state}`;
        sub.accessible_name = `${_('Accent colour')}: ${state}`;
        auto.setOrnament(on ? PopupMenu.Ornament.CHECK : PopupMenu.Ornament.NO_DOT);
        manual.setOrnament(on ? PopupMenu.Ornament.NO_DOT : PopupMenu.Ornament.CHECK);
    };
    sync();
    section.addMenuItem(sub);
    const id = appearance.connect(`changed::${ACCENT_KEY}`, sync);
    return () => { appearance.disconnect(id); };
}
