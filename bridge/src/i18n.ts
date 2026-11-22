// SPDX-License-Identifier: GPL-3.0-or-later
// Translations of the bridge (gettext domain "desktop-themes-rs", the same as the Rust programs).
// Source strings are English; po/uk.po in the repository carries the Ukrainian texts.
//
// language setting (io.github.OlehDzhumyk.Desk language):
//   'system' → the extension's gettext domain (Extension.initTranslations / gettext), i.e. the
//              shell's own locale;
//   'uk'     → the catalogue locale/uk.json that assemble.mjs builds from po/uk.po, so the panels
//              stay Ukrainian even when the session runs in English;
//   'en'     → the English source strings.
import type {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

let _ext: Extension | null = null;
let _lang = 'system';
let _catalog: Record<string, string> | null = null;

export function initI18n(ext: Extension, language: string): void {
    _ext = ext;
    try {
        ext.initTranslations();
    } catch (e) {
        console.warn(`desk-bridge: translations unavailable: ${e}`);
    }
    setLanguage(language);
}

export function setLanguage(language: string): void {
    _lang = ['uk', 'en'].includes(language) ? language : 'system';
    _catalog = null;
    if (_lang !== 'uk' || !_ext)
        return;
    try {
        const file = _ext.dir.get_child('locale').get_child(`${_lang}.json`);
        const [, bytes] = file.load_contents(null);
        _catalog = JSON.parse(new TextDecoder().decode(bytes)) as Record<string, string>;
    } catch (e) {
        console.warn(`desk-bridge: no catalogue for ${_lang}: ${e}`);
    }
}

/** Translate. Extracted by `xgettext -k_`. */
export function _(msgid: string): string {
    if (_lang === 'en')
        return msgid;
    if (_catalog)
        return _catalog[msgid] || msgid;
    return _ext ? _ext.gettext(msgid) : msgid;
}

/** Substitute named placeholders: fmt(template, {name}) replaces {name}. */
export function fmt(template: string, args: Record<string, string | number>): string {
    return template.replace(/\{(\w+)\}/g, (m, k: string) => (k in args ? String(args[k]) : m));
}

