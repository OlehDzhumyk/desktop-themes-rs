// SPDX-License-Identifier: GPL-3.0-or-later
// Bold name of the focused application next to the macOS-skin logo menu, with a small window menu
// (New Window, Hide, Hide Others, Quit). Owned by LogoMenuFeature: it exists only while the logo
// menu is shown.
import Clutter from 'gi://Clutter';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import {_} from '../i18n.js';
import {focusWindow} from '../shellTypes.js';

const ROLE = 'desk-app-menu';

/** Minimize the window that has the focus. */
function hideFocused(): void {
    focusWindow()?.minimize();
}

/** Minimize every other normal window on the active workspace. */
function hideOthers(): void {
    const focused = focusWindow();
    const ws = global.workspace_manager.get_active_workspace();
    for (const win of ws.list_windows()) {
        if (win === focused || win.minimized || win.skip_taskbar)
            continue;
        if (win.get_window_type() !== Meta.WindowType.NORMAL)
            continue;
        win.minimize();
    }
}

export class AppMenuButton {
    private _button: PanelMenu.Button;
    private _label: St.Label;
    private _tracker: Shell.WindowTracker;
    private _focusId = 0;

    /** Adds the button to the left box of the top bar at `position`. */
    constructor(position: number) {
        this._tracker = Shell.WindowTracker.get_default();
        this._button = new PanelMenu.Button(0.0, _('Application menu'), false);
        this._button.add_style_class_name('desk-app-menu');
        this._label = new St.Label({
            style_class: 'desk-app-menu-label',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._button.add_child(this._label);
        this._button.label_actor = this._label;

        if (this._button.menu instanceof PopupMenu.PopupMenu)
            this._button.menu.actor.add_style_class_name('desk-app-menu-popup');

        this._focusId = this._tracker.connect('notify::focus-app', () => { this._sync(); });
        this._sync();
        Main.panel.addToStatusArea(ROLE, this._button, position, 'left');
    }

    private _sync(): void {
        const app = this._tracker.focus_app;
        this._label.text = app ? app.get_name() : _('Desktop');
        // filled now, not on open: PopupMenu.open() refuses an empty menu
        if (this._button.menu instanceof PopupMenu.PopupMenu)
            this._fillMenu(this._button.menu);
    }

    private _fillMenu(menu: PopupMenu.PopupMenu): void {
        if (menu.isOpen)
            menu.close();
        menu.removeAll();
        const app = this._tracker.focus_app;
        if (!app) {
            // the desktop has the focus: only hiding the other windows makes sense
            menu.addAction(_('Hide Others'), () => { hideOthers(); });
            return;
        }
        if (app.can_open_new_window()) {
            menu.addAction(_('New Window'), () => { app.open_new_window(-1); });
            menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        }
        menu.addAction(_('Hide'), () => { hideFocused(); });
        menu.addAction(_('Hide Others'), () => { hideOthers(); });
        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        menu.addAction(_('Quit'), () => { app.request_quit(); });
    }

    destroy(): void {
        if (this._focusId)
            this._tracker.disconnect(this._focusId);
        this._focusId = 0;
        this._button.destroy();
    }
}
