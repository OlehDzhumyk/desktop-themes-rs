// SPDX-License-Identifier: GPL-3.0-or-later
// Lays out the installable extension in build/desk-bridge@desktop-themes-rs/:
// compiled JS (from tsc), metadata.json, stylesheets, the D-Bus interface XML and the
// GSettings schema shared with deskd (data/ in the repository root), compiled.
import {cpSync, mkdirSync, rmSync, readdirSync, copyFileSync} from 'node:fs';
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
console.log(`built ${out}`);
