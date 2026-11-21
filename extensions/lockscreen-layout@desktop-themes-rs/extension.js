// Lock Screen Layout: patches the GNOME Shell 51 UnlockDialog (clock position, fonts, blur, tint,
// dedicated background image). No network, no subprocesses; everything is restored in disable().
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Shell from 'gi://Shell';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as Layout from 'resource:///org/gnome/shell/ui/layout.js';
import {UnlockDialog} from 'resource:///org/gnome/shell/ui/unlockDialog.js';
import {Extension, InjectionManager} from 'resource:///org/gnome/shell/extensions/extension.js';

const A = Clutter.ActorAlign;

export default class LockscreenLayout extends Extension {
    enable() {
        this._s = this.getSettings();
        this._inj = new InjectionManager();
        const ext = this;
        this._inj.overrideMethod(UnlockDialog.prototype, '_init', orig => function (...args) {
            orig.apply(this, args);
            this.connect('destroy', () => ext._dropSources(this, true));
            ext._layout(this);
        });
        this._inj.overrideMethod(UnlockDialog.prototype, '_updateBackgroundEffects', orig => function (...args) {
            orig.apply(this, args);
            ext._effects(this);
        });
        this._changedId = this._s.connect('changed', () => this._refresh());
        this._refresh();
    }

    disable() {
        this._s.disconnect(this._changedId);
        this._inj.clear();   // stock methods are back before we touch the dialog
        this._inj = null;
        const d = Main.screenShield?._dialog;
        if (d) {
            this._unlayout(d);
            d._updateBackgrounds();          // fresh stock backgrounds + blur
            this._dropSources(d, true);
        }
        this._s = null;
    }

    _refresh() {
        const d = Main.screenShield?._dialog;
        if (!d) return;
        this._unlayout(d);
        this._layout(d);
        d._updateBackgroundEffects();
    }

    // ---- clock ----
    _layout(d) {
        const s = this._s, clock = d._clock, stack = d._stack;
        if (!clock || !stack || d._lx) return;
        const pos = s.get_string('clock-position');
        const size = s.get_int('clock-font-size'), dsize = s.get_int('date-font-size');
        const fam = s.get_string('clock-font-family'), wt = s.get_string('clock-font-weight');
        const font = fam ? `font-family: "${fam.replace(/["\\]/g, '')}";` : '';
        const color = k => s.get_string(k) ? `color: ${s.get_string(k)};` : '';
        clock._time.style = [size && `font-size: ${size}px;`, font, wt && `font-weight: ${wt};`, color('clock-color'), s.get_string('clock-glow') && `text-shadow: 0 0 28px ${s.get_string('clock-glow')};`].filter(Boolean).join(' ');
        clock._date.style = [dsize && `font-size: ${dsize}px;`, font, color('date-color')].filter(Boolean).join(' ');
        clock._hint.visible = !s.get_boolean('hide-hint');
        stack.style = `margin-top: ${2 * s.get_int('prompt-offset')}px;`;   // stack is centred, so 2x shifts the prompt by 1x
        d._lx = {};
        if (pos === 'default') return;

        // Move the clock out of the stack (a size-less stand-in keeps the stack maths valid) into a
        // monitor-sized layer where it can be aligned freely. St overrides actor margins from CSS,
        // so the margins go into the style.
        const left = pos === 'bottom-left';
        const mx = s.get_int('margin-x'), my = s.get_int('margin-y');
        const layer = new St.Widget({layout_manager: new Clutter.BinLayout()});
        layer.add_constraint(new Layout.MonitorConstraint({primary: true}));
        const standIn = new St.Widget();
        const index = stack.get_children().indexOf(clock);
        stack.remove_child(clock);
        stack._clock = standIn;
        layer.add_child(clock);
        d.add_child(layer);
        clock.style = left ? `margin-left: ${mx}px; margin-bottom: ${my}px;` : `margin-top: ${my}px;`;
        clock.set({x_expand: true, y_expand: true, x_align: left ? A.START : A.CENTER, y_align: left ? A.END : A.START});
        for (const l of [clock._time, clock._date, clock._hint])
            l.x_align = left ? A.START : A.CENTER;
        d._lx = {layer, standIn, index};
    }

    _unlayout(d) {
        const lx = d._lx, clock = d._clock, stack = d._stack;
        if (!lx) return;
        if (lx.layer) {
            lx.layer.remove_child(clock);
            stack.insert_child_at_index(clock, lx.index);
            stack._clock = clock;
            lx.layer.destroy();
            lx.standIn.destroy();
            clock.style = '';
            clock.set({x_expand: false, y_expand: false, x_align: A.FILL, y_align: A.FILL});
            for (const l of [clock._time, clock._date, clock._hint]) l.x_align = A.CENTER;
        }
        stack.style = '';
        clock._time.style = clock._date.style = '';
        clock._hint.visible = true;
        d._lx = null;
    }

    // ---- background ----
    _effects(d) {
        const s = this._s;
        if (!s || !d._backgroundGroup) return;
        const scale = St.ThemeContext.get_for_stage(global.stage).scale_factor;
        let uri = s.get_string('background-uri');
        if (uri === 'screensaver')   // GNOME 51 ignores this key itself; themes still set it
            uri = new Gio.Settings({schema_id: 'org.gnome.desktop.screensaver'}).get_string('picture-uri');
        const path = uri ? Gio.File.new_for_uri(uri).get_path() : null;
        const tint = s.get_string('tint');
        this._dropSources(d, false);
        d._backgroundGroup.get_children().forEach((w, i) => {
            if (path) this._useImage(d, d._bgManagers[i], path);
            if (!s.get_boolean('blur')) {
                w.remove_effect_by_name('blur');
            } else {
                let e = w.get_effect('blur');
                if (!e) w.add_effect(e = new Shell.BlurEffect({name: 'blur'}));
                e.set({radius: s.get_int('blur-radius') * scale, brightness: s.get_double('brightness')});
            }
            w._lxTint?.destroy();
            w._lxTint = null;
            if (tint) {   // after the background actor, so it paints on top (and inside the blur)
                const t = new St.Widget({style: `background-color: ${tint};`});
                t.add_constraint(new Clutter.BindConstraint({source: w, coordinate: Clutter.BindCoordinate.SIZE}));
                w.add_child(t);
                w._lxTint = t;
            }
        });
    }

    // A private BackgroundSource instance (same class the shell uses) with its image override set:
    // the shell's own scaling ("zoom" = cover), loading and caching are reused.
    _useImage(d, bgm, path) {
        if (!bgm) return;
        d._lxSrcs ??= [];
        let e = d._lxSrcs.find(x => x.bgm === bgm);
        if (e?.path === path) return;
        if (e) {
            bgm.backgroundActor?.destroy();
            e.src.destroy();
            d._lxSrcs.splice(d._lxSrcs.indexOf(e), 1);
        }
        const Source = bgm._backgroundSource.constructor;
        const src = new Source(Main.layoutManager, 'org.gnome.desktop.background');
        src._overrideImage = path;
        d._lxSrcs.push({bgm, src, path, orig: bgm._backgroundSource});
        bgm.backgroundActor?.destroy();
        bgm._backgroundSource = src;
        bgm.backgroundActor = bgm._createBackgroundActor();
    }

    // Destroy private sources whose manager is gone (or all of them when `all`).
    _dropSources(d, all) {
        if (!d._lxSrcs) return;
        d._lxSrcs = d._lxSrcs.filter(e => {
            if (!all && d._bgManagers?.includes(e.bgm)) return true;
            try {
                if (e.bgm.backgroundActor && e.bgm._backgroundSource === e.src) {
                    e.bgm.backgroundActor.destroy();
                    e.bgm.backgroundActor = null;
                    e.bgm._backgroundSource = e.orig;
                }
                e.src.destroy();
            } catch (err) {
                logError(err, 'lockscreen-layout: dropping background source');
            }
            return false;
        });
    }
}
