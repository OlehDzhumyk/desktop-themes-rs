// SPDX-License-Identifier: GPL-3.0-or-later
// Small skin-aware building blocks shared by the bridge surfaces (clipboard, emoji, colour picker,
// on-screen displays, text extractor): box orientations, the motion setting, the system style
// classes, a labelled icon button and an ellipsized label.
import Clutter from 'gi://Clutter';
import type Gio from 'gi://Gio';
import Pango from 'gi://Pango';
import St from 'gi://St';

export const VERTICAL = {orientation: Clutter.Orientation.VERTICAL};
export const HORIZONTAL = {orientation: Clutter.Orientation.HORIZONTAL};

/** Animations follow org.gnome.desktop.interface enable-animations and the reduced-motion setting. */
export function animationsEnabled(): boolean {
    const s = St.Settings.get();
    return s.enable_animations && s.reduced_motion !== St.ReducedMotion.REDUCE;
}

/**
 * Classes that let the skins follow the system (stylesheet.css):
 * .desk-light (colour scheme is not dark), .desk-accent-<colour> (GNOME accent colour) and
 * .desk-hc (high contrast: outlines, so nothing depends on colour alone).
 */
export function syncSystemClasses(widget: St.Widget): void {
    const st = St.Settings.get();
    const set = (cls: string, on: boolean) => {
        if (on)
            widget.add_style_class_name(cls);
        else
            widget.remove_style_class_name(cls);
    };
    set('desk-hc', st.high_contrast);
    set('desk-light', st.color_scheme !== St.SystemColorScheme.PREFER_DARK);
    const accents: [St.SystemAccentColor, string][] = [
        [St.SystemAccentColor.BLUE, 'blue'], [St.SystemAccentColor.TEAL, 'teal'],
        [St.SystemAccentColor.GREEN, 'green'], [St.SystemAccentColor.YELLOW, 'yellow'],
        [St.SystemAccentColor.ORANGE, 'orange'], [St.SystemAccentColor.RED, 'red'],
        [St.SystemAccentColor.PINK, 'pink'], [St.SystemAccentColor.PURPLE, 'purple'],
        [St.SystemAccentColor.SLATE, 'slate'],
    ];
    for (const [color, name] of accents)
        set(`desk-accent-${name}`, st.accent_color === color);
}

export function label(text: string, styleClass: string, lines = 1): St.Label {
    const l = new St.Label({text, style_class: styleClass, x_expand: true, y_align: Clutter.ActorAlign.CENTER});
    const ct = l.clutter_text;
    ct.ellipsize = Pango.EllipsizeMode.END;
    if (lines > 1) {
        ct.line_wrap = true;
        ct.line_wrap_mode = Pango.WrapMode.WORD_CHAR;
        // St has no max-lines: cap the height through the style sheet (line-height × lines)
    }
    return l;
}

export function iconButton(icon: string | Gio.Icon, accessible: string, styleClass: string,
    onClick: () => void): St.Button {
    const b = new St.Button({
        style_class: styleClass,
        can_focus: true,
        accessible_name: accessible,
        child: typeof icon === 'string'
            ? new St.Icon({icon_name: icon, style_class: 'desk-clip-button-icon'})
            : new St.Icon({gicon: icon, style_class: 'desk-clip-button-icon'}),
    });
    b.connect('clicked', () => { onClick(); });
    return b;
}
