// SPDX-License-Identifier: GPL-3.0-or-later
// Live keybinding reader for the Shortcut Guide (docs/spec/features/shortcut-guide.md): reads the
// currently active GSettings bindings at open time from GNOME and Desk schemas so theme bindings
// and user overrides always appear truthfully.
import Gio from 'gi://Gio';

import {type Skin} from '../accentPicker.js';
import {_} from '../i18n.js';

type GroupId = 'windows' | 'desktop' | 'features' | 'input' | 'custom';
type ShortcutOwner = 'theme' | 'user';

export interface ShortcutEntry {
    readonly group: GroupId;
    readonly action: string;
    /** Raw accelerator strings (e.g. `<Super>v`). */
    readonly accels: readonly string[];
    /** Formatted keycap token lists, one per accelerator. */
    readonly combos: readonly (readonly string[])[];
    readonly owner: ShortcutOwner;
}

export interface ShortcutGroup {
    readonly id: GroupId;
    readonly title: string;
    readonly entries: readonly ShortcutEntry[];
}

export interface ShortcutSnapshot {
    readonly groups: readonly ShortcutGroup[];
    /** True when one or more expected schemas could not be read. */
    readonly partial: boolean;
}

export type SchemaOpener = (id: string) => Gio.Settings | null;

interface StaticBindingSpec {
    readonly group: GroupId;
    readonly schema: string;
    readonly key: string;
    readonly label: () => string;
    readonly optionalSchema?: boolean;
}

interface DeskBindingSpec {
    readonly schema: string;
    readonly key: string;
    readonly overrideKey: string;
    readonly label: () => string;
}

const WM_SCHEMA = 'org.gnome.desktop.wm.keybindings';
const SHELL_SCHEMA = 'org.gnome.shell.keybindings';
const MEDIA_SCHEMA = 'org.gnome.settings-daemon.plugins.media-keys';
const CUSTOM_SCHEMA = 'org.gnome.settings-daemon.plugins.media-keys.custom-keybinding';
const MUTTER_SCHEMA = 'org.gnome.mutter.keybindings';
const SHORTCUTS_SCHEMA = 'io.github.OlehDzhumyk.Desk.Shortcuts';

const GNOME_BINDINGS: readonly StaticBindingSpec[] = [
    // Windows and apps
    {group: 'windows', schema: WM_SCHEMA, key: 'switch-applications', label: () => _('Switch applications')},
    {group: 'windows', schema: WM_SCHEMA, key: 'switch-windows', label: () => _('Switch windows')},
    {group: 'windows', schema: WM_SCHEMA, key: 'switch-group', label: () => _('Switch windows of an app')},
    {group: 'windows', schema: WM_SCHEMA, key: 'close', label: () => _('Close window')},
    {group: 'windows', schema: WM_SCHEMA, key: 'minimize', label: () => _('Minimise window')},
    {group: 'windows', schema: WM_SCHEMA, key: 'toggle-maximized', label: () => _('Maximise window')},
    {group: 'windows', schema: WM_SCHEMA, key: 'toggle-fullscreen', label: () => _('Toggle full screen')},
    {group: 'windows', schema: WM_SCHEMA, key: 'show-desktop', label: () => _('Show desktop')},
    {group: 'windows', schema: WM_SCHEMA, key: 'begin-move', label: () => _('Move window')},
    {group: 'windows', schema: WM_SCHEMA, key: 'begin-resize', label: () => _('Resize window')},
    {
        group: 'windows',
        schema: MUTTER_SCHEMA,
        key: 'toggle-tiled-left',
        label: () => _('Tile window left'),
        optionalSchema: true,
    },
    {
        group: 'windows',
        schema: MUTTER_SCHEMA,
        key: 'toggle-tiled-right',
        label: () => _('Tile window right'),
        optionalSchema: true,
    },

    // Desktop
    {group: 'desktop', schema: SHELL_SCHEMA, key: 'toggle-overview', label: () => _('Activities overview')},
    {group: 'desktop', schema: SHELL_SCHEMA, key: 'toggle-application-view', label: () => _('Show all apps')},
    {group: 'desktop', schema: SHELL_SCHEMA, key: 'toggle-message-tray', label: () => _('Notifications and calendar')},
    {group: 'desktop', schema: SHELL_SCHEMA, key: 'toggle-quick-settings', label: () => _('Quick settings')},
    {group: 'desktop', schema: SHELL_SCHEMA, key: 'focus-active-notification', label: () => _('Focus notification')},
    {group: 'desktop', schema: WM_SCHEMA, key: 'panel-run-dialog', label: () => _('Run a command')},
    {group: 'desktop', schema: WM_SCHEMA, key: 'switch-to-workspace-left', label: () => _('Previous workspace')},
    {group: 'desktop', schema: WM_SCHEMA, key: 'switch-to-workspace-right', label: () => _('Next workspace')},
    {
        group: 'desktop',
        schema: WM_SCHEMA,
        key: 'move-to-workspace-left',
        label: () => _('Move window to previous workspace'),
    },
    {
        group: 'desktop',
        schema: WM_SCHEMA,
        key: 'move-to-workspace-right',
        label: () => _('Move window to next workspace'),
    },
    {group: 'desktop', schema: MEDIA_SCHEMA, key: 'screensaver', label: () => _('Lock screen')},
    {group: 'desktop', schema: MEDIA_SCHEMA, key: 'home', label: () => _('Open Files')},
    {group: 'desktop', schema: MEDIA_SCHEMA, key: 'control-center', label: () => _('Open Settings')},
    {group: 'desktop', schema: MEDIA_SCHEMA, key: 'www', label: () => _('Open web browser')},
    {group: 'desktop', schema: MEDIA_SCHEMA, key: 'email', label: () => _('Open email')},
    {group: 'desktop', schema: MEDIA_SCHEMA, key: 'terminal', label: () => _('Open terminal')},

    // GNOME built-in capture shortcuts (appear when active, e.g. in default/Fedora)
    {group: 'features', schema: SHELL_SCHEMA, key: 'show-screenshot-ui', label: () => _('GNOME screenshot')},
    {group: 'features', schema: SHELL_SCHEMA, key: 'show-screen-recording-ui', label: () => _('GNOME screen recording')},

    // Input
    {group: 'input', schema: WM_SCHEMA, key: 'switch-input-source', label: () => _('Next input source')},
    {group: 'input', schema: WM_SCHEMA, key: 'switch-input-source-backward', label: () => _('Previous input source')},
];

const DESK_BINDINGS: readonly DeskBindingSpec[] = [
    {
        schema: 'io.github.OlehDzhumyk.Desk.Clipboard',
        key: 'toggle-shortcut',
        overrideKey: 'clipboard-toggle',
        label: () => _('Clipboard history'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Emoji',
        key: 'emoji-shortcut',
        overrideKey: 'emoji-toggle',
        label: () => _('Emoji and symbols'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Mixer',
        key: 'mixer-shortcut',
        overrideKey: 'mixer-open',
        label: () => _('Volume mixer'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.ColorPicker',
        key: 'color-picker-shortcut',
        overrideKey: 'color-picker-toggle',
        label: () => _('Colour picker'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Ocr',
        key: 'ocr-shortcut',
        overrideKey: 'ocr-toggle',
        label: () => _('Text extractor'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Capture',
        key: 'capture-region-shortcut',
        overrideKey: 'capture-region',
        label: () => _('Capture region'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Capture',
        key: 'capture-screen-shortcut',
        overrideKey: 'capture-screen',
        label: () => _('Capture screen'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Capture',
        key: 'capture-toolbar-shortcut',
        overrideKey: 'capture-toolbar',
        label: () => _('Screenshot toolbar'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.Capture',
        key: 'capture-record-shortcut',
        overrideKey: 'capture-record',
        label: () => _('Record screen'),
    },
    {
        schema: 'io.github.OlehDzhumyk.Desk.ShortcutGuide',
        key: 'shortcut-guide-shortcut',
        overrideKey: 'shortcut-guide-toggle',
        label: () => _('Shortcut guide'),
    },
];

const GROUP_ORDER: readonly GroupId[] = ['windows', 'desktop', 'features', 'input', 'custom'];

function groupTitle(id: GroupId): string {
    switch (id) {
    case 'windows':
        return _('Windows and apps');
    case 'desktop':
        return _('Desktop');
    case 'features':
        return _('Features');
    case 'input':
        return _('Input');
    case 'custom':
        return _('Custom');
    }
}

function isDisabled(accel: string): boolean {
    const trimmed = accel.trim();
    return trimmed.length === 0 || trimmed.toLowerCase() === 'disabled';
}

function formatKeyName(rawKey: string, skin: Skin): string {
    const mac = skin === 'macos';
    switch (rawKey) {
    case 'Return':
    case 'KP_Enter':
        return mac ? '↵' : 'Enter';
    case 'Escape':
        return 'Esc';
    case 'BackSpace':
        return mac ? '⌫' : 'Backspace';
    case 'Delete':
        return mac ? '⌦' : 'Del';
    case 'Tab':
    case 'ISO_Left_Tab':
        return mac ? '⇥' : 'Tab';
    case 'space':
        return 'Space';
    case 'Left':
        return '←';
    case 'Right':
        return '→';
    case 'Up':
        return '↑';
    case 'Down':
        return '↓';
    case 'Page_Up':
    case 'Prior':
        return 'PgUp';
    case 'Page_Down':
    case 'Next':
        return 'PgDn';
    case 'Home':
        return 'Home';
    case 'End':
        return 'End';
    case 'Print':
        return 'PrtSc';
    case 'slash':
        return '/';
    case 'backslash':
        return '\\';
    case 'period':
        return '.';
    case 'comma':
        return ',';
    case 'semicolon':
        return ';';
    case 'colon':
        return ':';
    case 'minus':
        return '-';
    case 'plus':
        return '+';
    case 'equal':
        return '=';
    case 'grave':
    case 'Above_Tab':
        return '`';
    case 'Super_L':
    case 'Super_R':
        return mac ? '⌘' : 'Super';
    default:
        if (rawKey.length === 1)
            return rawKey.toUpperCase();
        if (rawKey.startsWith('XF86'))
            return rawKey.slice(4);
        return rawKey;
    }
}

/** Parse a GTK/GNOME accelerator string into display keycap tokens for the active skin. */
function formatAccel(accel: string, skin: Skin): string[] {
    const mac = skin === 'macos';
    const tokens: string[] = [];
    const modRegex = /<([^>]+)>/g;
    let lastIndex = 0;
    let match: RegExpExecArray | null = modRegex.exec(accel);
    while (match !== null) {
        const mod = (match[1] ?? '').toLowerCase();
        lastIndex = modRegex.lastIndex;
        if (mod === 'control' || mod === 'ctrl' || mod === 'primary')
            tokens.push(mac ? '⌃' : 'Ctrl');
        else if (mod === 'alt' || mod === 'mod1')
            tokens.push(mac ? '⌥' : 'Alt');
        else if (mod === 'shift')
            tokens.push(mac ? '⇧' : 'Shift');
        else if (mod === 'super' || mod === 'meta' || mod === 'mod4')
            tokens.push(mac ? '⌘' : 'Super');
        else if (mod.length > 0)
            tokens.push(match[1] ?? mod);
        match = modRegex.exec(accel);
    }
    const keyPart = accel.slice(lastIndex).trim();
    if (keyPart.length > 0)
        tokens.push(formatKeyName(keyPart, skin));
    return tokens;
}

function readStrv(settings: Gio.Settings, key: string): string[] {
    if (!settings.settings_schema.has_key(key))
        return [];
    try {
        return settings.get_strv(key).filter(v => !isDisabled(v));
    } catch {
        return [];
    }
}

function readCustomShortcuts(mediaSettings: Gio.Settings, skin: Skin): ShortcutEntry[] {
    const paths = readStrv(mediaSettings, 'custom-keybindings');
    if (paths.length === 0)
        return [];
    const defaultSource = Gio.SettingsSchemaSource.get_default();
    const customSchema = defaultSource?.lookup(CUSTOM_SCHEMA, true);
    if (!customSchema)
        return [];
    const result: ShortcutEntry[] = [];
    for (const path of paths) {
        try {
            const item = new Gio.Settings({settings_schema: customSchema, path});
            const binding = item.get_string('binding');
            if (isDisabled(binding))
                continue;
            const rawName = item.get_string('name').trim();
            const action = rawName.length > 0 ? rawName : _('Custom shortcut');
            const combos = [formatAccel(binding, skin)].filter(c => c.length > 0);
            if (combos.length === 0)
                continue;
            result.push({
                group: 'custom',
                action,
                accels: [binding],
                combos,
                owner: 'user',
            });
        } catch {
            // skip malformed custom keybinding paths
        }
    }
    return result;
}

/** Read all live active shortcuts grouped and formatted for `skin`. */
export function readLiveShortcuts(openSchema: SchemaOpener, skin: Skin): ShortcutSnapshot {
    const cache = new Map<string, Gio.Settings | null>();
    let partial = false;

    const getSchema = (id: string, optional = false): Gio.Settings | null => {
        if (cache.has(id))
            return cache.get(id) ?? null;
        let loaded: Gio.Settings | null;
        try {
            loaded = openSchema(id);
        } catch {
            loaded = null;
        }
        if (!loaded && !optional)
            partial = true;
        cache.set(id, loaded);
        return loaded;
    };

    const buckets = new Map<GroupId, ShortcutEntry[]>();
    for (const gid of GROUP_ORDER)
        buckets.set(gid, []);

    const pushEntry = (
        group: GroupId,
        action: string,
        accels: string[],
        owner: ShortcutOwner,
    ): void => {
        if (accels.length === 0)
            return;
        const combos = accels.map(a => formatAccel(a, skin)).filter(c => c.length > 0);
        if (combos.length === 0)
            return;
        buckets.get(group)?.push({group, action, accels, combos, owner});
    };

    // 1. GNOME system keybindings
    for (const spec of GNOME_BINDINGS) {
        const settings = getSchema(spec.schema, spec.optionalSchema === true);
        if (!settings)
            continue;
        const accels = readStrv(settings, spec.key);
        pushEntry(spec.group, spec.label(), accels, 'theme');
    }

    // 2. Desk feature keybindings + user overrides in Desk.Shortcuts
    const overrides = getSchema(SHORTCUTS_SCHEMA, true);
    for (const spec of DESK_BINDINGS) {
        const settings = getSchema(spec.schema, true);
        if (!settings)
            continue;
        if (settings.settings_schema.has_key('enabled') && !settings.get_boolean('enabled'))
            continue;
        const overrideAccels = overrides ? readStrv(overrides, spec.overrideKey) : [];
        const hasExplicitDisabledOverride = overrides !== null
            && overrides.settings_schema.has_key(spec.overrideKey)
            && overrides.get_strv(spec.overrideKey).some(v => v.trim().toLowerCase() === 'disabled');
        if (hasExplicitDisabledOverride)
            continue;
        const activeAccels = overrideAccels.length > 0
            ? overrideAccels
            : readStrv(settings, spec.key);
        const owner: ShortcutOwner = overrideAccels.length > 0 ? 'user' : 'theme';
        pushEntry('features', spec.label(), activeAccels, owner);
    }

    // 3. Custom shortcuts from media-keys
    const mediaSettings = getSchema(MEDIA_SCHEMA, false);
    if (mediaSettings) {
        const customEntries = readCustomShortcuts(mediaSettings, skin);
        const customBucket = buckets.get('custom');
        if (customBucket)
            customBucket.push(...customEntries);
    }

    const groups: ShortcutGroup[] = [];
    for (const gid of GROUP_ORDER) {
        const entries = buckets.get(gid) ?? [];
        if (entries.length === 0)
            continue;
        groups.push({
            id: gid,
            title: groupTitle(gid),
            entries,
        });
    }

    return {groups, partial};
}
