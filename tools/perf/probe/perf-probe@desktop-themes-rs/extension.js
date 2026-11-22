// SPDX-License-Identifier: GPL-3.0-or-later
// Test-only probe loaded into the ISOLATED headless shell by tools/perf (never installed for users).
// It owns org.desktopthemesrs.PerfProbe on the private bus and times the clipboard panel of
// desk-bridge without any change to the bridge: Toggle() opens the panel exactly like the keyboard
// shortcut does (ClipboardFeature.toggle()) and reports, in milliseconds since the call arrived,
//   visible   the panel actor is visible (open() returned),
//   rendered  the list of items from deskd was rendered (List round trip + render),
//   painted   the first frame after that was painted by the compositor (after-paint).
// then closes the panel again.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const BRIDGE = 'desk-bridge@desktop-themes-rs';
const XML = `<node><interface name="org.desktopthemesrs.PerfProbe">
  <method name="Toggle"><arg type="d" direction="out" name="visible_ms"/>
    <arg type="d" direction="out" name="rendered_ms"/><arg type="d" direction="out" name="painted_ms"/></method>
  <method name="Skin"><arg type="s" direction="out" name="skin"/></method>
</interface></node>`;

const now = () => GLib.get_monotonic_time() / 1000;

export default class PerfProbe extends Extension {
    enable() {
        this._impl = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._impl.export(Gio.DBus.session, '/org/desktopthemesrs/PerfProbe');
        this._own = Gio.bus_own_name_on_connection(Gio.DBus.session, 'org.desktopthemesrs.PerfProbe',
            Gio.BusNameOwnerFlags.NONE, null, null);
    }

    disable() {
        this._impl?.unexport();
        this._impl = null;
        if (this._own)
            Gio.bus_unown_name(this._own);
        this._own = 0;
    }

    _feature() {
        return Main.extensionManager.lookup(BRIDGE)?.stateObj?._clipboard ?? null;
    }

    Skin() {
        return this._feature()?.skin ?? '';
    }

    ToggleAsync(_params, invocation) {
        const feature = this._feature();
        const panel = feature?._panel;
        if (!panel) {
            invocation.return_dbus_error('org.desktopthemesrs.PerfProbe.Error', 'desk-bridge clipboard panel not available');
            return;
        }
        const t0 = now();
        let rendered = 0;
        const origRender = panel.render;
        panel.render = function (...a) {
            origRender.apply(this, a);
            if (!rendered)
                rendered = now();
        };
        panel.open();
        const visible = panel.isOpen ? now() : 0;
        const finish = painted => {
            panel.render = origRender;
            panel.close();
            invocation.return_value(new GLib.Variant('(ddd)', [visible - t0, rendered - t0, painted - t0]));
        };
        // wait until the list is rendered, then for the next painted frame
        let tries = 0;
        const poll = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1, () => {
            if (rendered || ++tries > 3000) {
                const id = global.stage.connect('after-paint', () => {
                    global.stage.disconnect(id);
                    finish(now());
                });
                panel.actor.queue_redraw();
                return GLib.SOURCE_REMOVE;
            }
            return GLib.SOURCE_CONTINUE;
        });
        void poll;
    }
}
