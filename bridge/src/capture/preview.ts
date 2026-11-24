// SPDX-License-Identifier: GPL-3.0-or-later
// What the user sees after a capture (docs/spec/features/screenshots.md, AC-SHOT-04).
// macos: a floating thumbnail at the bottom right of the current monitor for 6 s. It stays while
// the pointer is on it; a click opens the editor; a right click, or Esc while it has the focus,
// dismisses it. It slides in (<= 150 ms) only when animations are enabled (AC-SHOT-09).
// windows11 and default: the shared toast («Screenshot saved»).
import Atk from 'gi://Atk';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {_} from '../i18n.js';
import {animationsEnabled, syncSystemClasses} from '../ui/primitives.js';
import {DeskToast} from '../ui/toast.js';

const THUMBNAIL_MS = 6000;
const TOAST_MS = 2500;
const SLIDE_MS = 150;
/** The long side of the thumbnail, in logical pixels. */
const THUMBNAIL_SIZE = 180;
const MARGIN = 24;

export class CapturePreview {
    private _thumb: St.Button | null = null;
    private _hideId = 0;
    private _toast: DeskToast | null = null;

    /** The toast of the windows11 and default skins (also used for errors and the countdown). */
    toast(): DeskToast {
        // no fade: hideAll() must remove it at once, before pixels are read
        this._toast ??= new DeskToast();
        return this._toast;
    }

    /** The confirmation of a saved capture in the skin's style. */
    showSaved(path: string, copied: boolean, skin: string, onEdit: (path: string) => void): void {
        if (skin === 'macos') {
            this._showThumbnail(path, skin, onEdit);
            return;
        }
        this.toast().show(copied ? _('Screenshot copied and saved') : _('Screenshot saved'), skin, TOAST_MS);
    }

    private _showThumbnail(path: string, skin: string, onEdit: (path: string) => void): void {
        this.hideThumbnail();
        const icon = new St.Icon({
            gicon: new Gio.FileIcon({file: Gio.File.new_for_path(path)}),
            icon_size: THUMBNAIL_SIZE,
            style_class: 'desk-capture-thumbnail-image',
        });
        const thumb = new St.Button({
            style_class: `desk-capture-thumbnail desk-skin-${skin}`,
            reactive: true,
            can_focus: true,
            track_hover: true,
            button_mask: St.ButtonMask.PRIMARY | St.ButtonMask.SECONDARY,
            accessible_role: Atk.Role.PUSH_BUTTON,
            accessible_name: _('Screenshot saved. Open it in the editor'),
            child: icon,
        });
        syncSystemClasses(thumb);
        thumb.connect('clicked', (_b: St.Button, button: number) => {
            this.hideThumbnail();
            if (button !== Clutter.BUTTON_SECONDARY)
                onEdit(path);
        });
        thumb.connect('key-press-event', (_a: Clutter.Actor, ev: Clutter.Event) => {
            if (ev.get_key_symbol() !== Clutter.KEY_Escape)
                return Clutter.EVENT_PROPAGATE;
            this.hideThumbnail();
            return Clutter.EVENT_STOP;
        });
        // the thumbnail stays while the pointer is on it
        thumb.connect('notify::hover', () => {
            if (thumb.hover)
                this._cancelHide();
            else
                this._scheduleHide();
        });
        Main.layoutManager.addTopChrome(thumb);
        this._thumb = thumb;
        const m = Main.layoutManager.currentMonitor;
        if (m) {
            const [, natW] = thumb.get_preferred_width(-1);
            const [, natH] = thumb.get_preferred_height(natW);
            thumb.set_position(m.x + m.width - natW - MARGIN, m.y + m.height - natH - MARGIN);
        }
        if (animationsEnabled()) {
            thumb.translation_x = THUMBNAIL_SIZE / 2;
            thumb.opacity = 0;
            thumb.ease({translationX: 0, opacity: 255, duration: SLIDE_MS, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
        }
        this._scheduleHide();
    }

    private _scheduleHide(): void {
        this._cancelHide();
        this._hideId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, THUMBNAIL_MS, () => {
            this._hideId = 0;
            this.hideThumbnail();
            return GLib.SOURCE_REMOVE;
        });
    }

    private _cancelHide(): void {
        if (this._hideId)
            GLib.source_remove(this._hideId);
        this._hideId = 0;
    }

    /** Remove the thumbnail at once (also before a new capture, so it is not in the image). */
    hideThumbnail(): void {
        this._cancelHide();
        const t = this._thumb;
        this._thumb = null;
        if (!t)
            return;
        t.remove_all_transitions();
        if (t.get_parent())
            Main.layoutManager.removeChrome(t);
        t.destroy();
    }

    /** Hide everything before pixels are read. */
    hideAll(): void {
        this.hideThumbnail();
        this._toast?.hide();
    }

    destroy(): void {
        this.hideThumbnail();
        this._toast?.destroy();
        this._toast = null;
    }
}
