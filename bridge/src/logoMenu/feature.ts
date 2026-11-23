// SPDX-License-Identifier: GPL-3.0-or-later
// Logo menu in the top bar for the macOS skin (replaces the third-party logomenu@aryan_k extension).
// Active only when skin === 'macos' and io.github.OlehDzhumyk.Desk.LogoMenu enabled is true.
import type Gio from 'gi://Gio';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {_} from '../i18n.js';
import {
    forceQuitFocused,
    lockScreen,
    logOut,
    openAbout,
    openSettings,
    restart,
    showApps,
    shutDown,
    suspend,
} from './actions.js';

const ROLE = 'desk-logo-menu';

export class LogoMenuFeature {
    private _settings: Gio.Settings;
    private _skin: string;
    private _button: PanelMenu.Button | null = null;
    private _savedActivitiesVisible: boolean | null = null;
    private _enabledId = 0;
    private _hideId = 0;

    constructor(settings: Gio.Settings, skin: string) {
        this._settings = settings;
        this._skin = skin;
        this._enabledId = this._settings.connect('changed::enabled', () => { this._sync(); });
        this._hideId = this._settings.connect('changed::hide-activities', () => { this._sync(); });
        this._sync();
    }

    setSkin(skin: string): void {
        if (this._skin === skin)
            return;
        this._skin = skin;
        this._sync();
    }

    private _shouldShow(): boolean {
        return this._skin === 'macos' && this._settings.get_boolean('enabled');
    }

    private _sync(): void {
        if (this._shouldShow()) {
            if (!this._button)
                this._attach();
        } else {
            this._detach();
        }
        this._syncActivities();
    }

    private _attach(): void {
        const btn = new PanelMenu.Button(0.0, _('Logo menu'), false);
        btn.add_child(new St.Icon({
            icon_name: 'start-here-symbolic',
            style_class: 'system-status-icon desk-logo-menu-icon',
        }));

        if (btn.menu instanceof PopupMenu.PopupMenu) {
            btn.menu.addAction(_('About This System'), () => { openAbout(); });
            btn.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            btn.menu.addAction(_('System Settings…'), () => { openSettings(); });
            btn.menu.addAction(_('Applications'), () => { showApps(); });
            btn.menu.addAction(_('Force Quit…'), () => { void forceQuitFocused(); });
            btn.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            btn.menu.addAction(_('Sleep'), () => { suspend(); });
            btn.menu.addAction(_('Restart…'), () => { restart(); });
            btn.menu.addAction(_('Shut Down…'), () => { shutDown(); });
            btn.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            btn.menu.addAction(_('Lock Screen'), () => { lockScreen(); });
            btn.menu.addAction(_('Log Out…'), () => { logOut(); });
        }

        Main.panel.addToStatusArea(ROLE, btn, 0, 'left');
        this._button = btn;
    }

    private _detach(): void {
        if (this._button) {
            this._button.destroy();
            this._button = null;
        }
    }

    private _syncActivities(): void {
        const activities = Main.panel.statusArea.activities;
        if (!activities)
            return;
        const shouldHide = this._button !== null && this._settings.get_boolean('hide-activities');
        if (shouldHide) {
            this._savedActivitiesVisible ??= activities.visible;
            activities.visible = false;
        } else if (this._savedActivitiesVisible !== null) {
            activities.visible = this._savedActivitiesVisible;
            this._savedActivitiesVisible = null;
        }
    }

    destroy(): void {
        for (const id of [this._enabledId, this._hideId]) {
            if (id)
                this._settings.disconnect(id);
        }
        this._enabledId = 0;
        this._hideId = 0;
        this._detach();
        this._syncActivities();
    }
}
