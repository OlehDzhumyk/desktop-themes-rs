// SPDX-License-Identifier: GPL-3.0-or-later
// Lays out the installable extension in build/desk-bridge@desktop-themes-rs/:
// compiled JS (from tsc), metadata.json, stylesheets, the D-Bus interface XML and the
// GSettings schema shared with deskd (data/ in the repository root), compiled, and the translations
// from po/: locale/<lang>/LC_MESSAGES/desktop-themes-rs.mo (msgfmt, used when the language setting
// is 'system') and locale/<lang>.json (used when the setting names a language).
import {cpSync, mkdirSync, rmSync, readdirSync, copyFileSync, readFileSync, writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {join, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..');
const uuid = 'desk-bridge@desktop-themes-rs';
const out = join(here, 'build', uuid);
rmSync(out, {recursive: true, force: true});
mkdirSync(out, {recursive: true});
cpSync(join(here, 'build', 'js'), out, {recursive: true});
for (const f of readdirSync(join(here, 'static')))
    cpSync(join(here, 'static', f), join(out, f), {recursive: true});
mkdirSync(join(out, 'dbus'));
for (const f of readdirSync(join(repo, 'data', 'dbus')))
    copyFileSync(join(repo, 'data', 'dbus', f), join(out, 'dbus', f));
mkdirSync(join(out, 'schemas'));
copyFileSync(join(repo, 'data', 'io.github.OlehDzhumyk.Desk.gschema.xml'),
    join(out, 'schemas', 'io.github.OlehDzhumyk.Desk.gschema.xml'));
execFileSync('glib-compile-schemas', ['--strict', join(out, 'schemas')], {stdio: 'inherit'});

/** msgid → msgstr of a .po file (translated, non-fuzzy, singular entries). */
function parsePo(text) {
    const out = {};
    const unq = l => JSON.parse(l.slice(l.indexOf('"')));
    for (const block of text.split(/\n\s*\n/)) {
        if (/^#,.*\bfuzzy\b/m.test(block))
            continue;
        let cur = null;
        const e = {msgid: '', msgstr: ''};
        for (const line of block.split('\n')) {
            if (line.startsWith('msgid '))
                cur = 'msgid';
            else if (line.startsWith('msgstr ') || line.startsWith('msgstr[0] '))
                cur = 'msgstr';
            else if (line.startsWith('msgid_plural') || line.startsWith('msgstr[') || line.startsWith('msgctxt'))
                cur = null;
            else if (!line.startsWith('"'))
                continue;
            if (cur)
                e[cur] += unq(line);
        }
        if (e.msgid && e.msgstr)
            out[e.msgid] = e.msgstr;
    }
    return out;
}

const po = join(repo, 'po');
for (const f of readdirSync(po).filter(n => n.endsWith('.po'))) {
    const lang = f.slice(0, -3);
    const dir = join(out, 'locale', lang, 'LC_MESSAGES');
    mkdirSync(dir, {recursive: true});
    try {
        execFileSync('msgfmt', ['--check', '-o', join(dir, 'desktop-themes-rs.mo'), join(po, f)], {stdio: 'inherit'});
    } catch (e) {
        // without gettext tools only the JSON catalogue is built (used when language = uk)
        console.warn(`msgfmt unavailable, ${lang}.mo skipped: ${e.message}`);
    }
    writeFileSync(join(out, 'locale', `${lang}.json`), JSON.stringify(parsePo(readFileSync(join(po, f), 'utf8'))));
}
console.log(`built ${out}`);
