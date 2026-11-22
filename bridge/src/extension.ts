// SPDX-License-Identifier: GPL-3.0-or-later
// desk-bridge@desktop-themes-rs — the one GNOME Shell extension of desktop-themes-rs.
// It renders shell-side UI and forwards events; the logic and state live in deskd
// (io.github.OlehDzhumyk.Desk) and in the theme service (io.github.OlehDzhumyk.Themes).
//
// Skins: the GSettings key io.github.OlehDzhumyk.Desk skin ('default' | 'macos' | 'windows11'),
// which themes set in their look module (desk.dconf), picks the UI variant and the stylesheet class.
import Gio from 'gi://Gio';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {ClipboardFeature} from './clipboard/feature.js';
import {DeskIndicator, launchGui} from './quickSettings.js';

const CLIPBOARD_SCHEMA = 'io.github.OlehDzhumyk.Desk.Clipboard';

export default class DeskBridge extends Extension {
    private _settings: Gio.Settings | null = null;
    private _clipSettings: Gio.Settings | null = null;
    private _indicator: InstanceType<typeof DeskIndicator> | null = null;
    private _clipboard: ClipboardFeature | null = null;
    private _skinId = 0;

    /** Settings from the extension's own schemas/ (falls back to the system/user schema dirs). */
    private _schema(id: string): Gio.Settings {
        const dir = this.dir.get_child('schemas');
        let source = Gio.SettingsSchemaSource.get_default();
        if (dir.query_exists(null))
            source = Gio.SettingsSchemaSource.new_from_directory(dir.get_path()!, source, false);
        const schema = source?.lookup(id, true);
        if (!schema)
            throw new Error(`GSettings schema ${id} not found`);
        return new Gio.Settings({settings_schema: schema});
    }

    override enable(): void {
        this._settings = this._schema('io.github.OlehDzhumyk.Desk');
        this._clipSettings = this._schema(CLIPBOARD_SCHEMA);
        this._indicator = new DeskIndicator();
        Main.panel.statusArea.quickSettings!.addExternalIndicator(this._indicator);
        this._clipboard = new ClipboardFeature(this.path, this._clipSettings, this._skin(),
            () => launchGui('clipboard'));
        this._applySkin();
        this._skinId = this._settings.connect('changed::skin', () => this._applySkin());
    }

    private _skin(): string {
        const s = this._settings?.get_string('skin') ?? 'default';
        return ['macos', 'windows11'].includes(s) ? s : 'default';
    }

    private _applySkin(): void {
        const skin = this._skin();
        this._clipboard?.setSkin(skin);
        this._indicator?.setClipboard(skin === 'default' ? this._clipboard : null);
    }

    override disable(): void {
        if (this._skinId)
            this._settings?.disconnect(this._skinId);
        this._skinId = 0;
        this._indicator?.setClipboard(null);
        this._clipboard?.destroy();
        this._clipboard = null;
        this._indicator?.destroy();
        this._indicator = null;
        this._settings = null;
        this._clipSettings = null;
    }
}
