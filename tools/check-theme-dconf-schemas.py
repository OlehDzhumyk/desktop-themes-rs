#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Verify that every key written by themes/*/*.dconf exists in its GSettings schema.

Loads <schema path="..."> -> <key name="..."> from:
  - data/*.gschema.xml
  - extensions/*/schemas/*.gschema.xml
  - /usr/share/glib-2.0/schemas/*.gschema.xml
  - ~/.local/share/gnome-shell/extensions/*/schemas/*.gschema.xml
  - cached extension ZIPs in ~/.cache/desktop-themes/ego/*.zip

Schemas whose path is not installed/cached (and relocatable schemas) are counted as skipped.
Any unknown key in a known schema fails with exit code 1.
"""

from __future__ import annotations

import os
import pathlib
import sys
import xml.etree.ElementTree as ET
import zipfile


def normalize_path(path: str) -> str:
    p = path.strip()
    if not p.startswith("/"):
        p = "/" + p
    if not p.endswith("/"):
        p = p + "/"
    return p


def parse_gschema_xml(
    data: bytes,
    schemas_by_id: dict[str, tuple[str | None, set[str], list[tuple[str, str]]]],
) -> None:
    try:
        root = ET.fromstring(data)
    except ET.ParseError:
        return
    for schema in root.findall(".//schema"):
        sid = schema.get("id")
        if not sid:
            continue
        raw_path = schema.get("path")
        path = normalize_path(raw_path) if raw_path else None
        keys = {
            k.get("name")
            for k in schema.findall("key")
            if k.get("name")
        }
        children = [
            (c.get("name"), c.get("schema"))
            for c in schema.findall("child")
            if c.get("name") and c.get("schema")
        ]
        prev = schemas_by_id.get(sid)
        if prev is None:
            schemas_by_id[sid] = (path, keys, children)
        else:
            prev_path, prev_keys, prev_children = prev
            schemas_by_id[sid] = (
                path or prev_path,
                prev_keys | keys,
                prev_children + [c for c in children if c not in prev_children],
            )


def load_schemas(repo_root: pathlib.Path) -> dict[str, set[str]]:
    schemas_by_id: dict[str, tuple[str | None, set[str], list[tuple[str, str]]]] = {}

    home = pathlib.Path.home()
    data_home = pathlib.Path(os.environ.get("XDG_DATA_HOME", home / ".local/share"))
    cache_home = pathlib.Path(os.environ.get("XDG_CACHE_HOME", home / ".cache"))

    xml_globs = [
        (repo_root / "data", "*.gschema.xml"),
        (repo_root / "extensions", "*/schemas/*.gschema.xml"),
        (pathlib.Path("/usr/share/glib-2.0/schemas"), "*.gschema.xml"),
        (data_home / "gnome-shell/extensions", "*/schemas/*.gschema.xml"),
    ]
    for base, pattern in xml_globs:
        if not base.is_dir():
            continue
        for xml_path in sorted(base.glob(pattern)):
            if not xml_path.is_file():
                continue
            try:
                parse_gschema_xml(xml_path.read_bytes(), schemas_by_id)
            except OSError:
                continue

    ego_cache = cache_home / "desktop-themes/ego"
    if ego_cache.is_dir():
        for zip_path in sorted(ego_cache.glob("*.zip")):
            try:
                with zipfile.ZipFile(zip_path) as zf:
                    for name in zf.namelist():
                        if name.endswith(".gschema.xml"):
                            parse_gschema_xml(zf.read(name), schemas_by_id)
            except (OSError, zipfile.BadZipFile):
                continue

    by_path: dict[str, set[str]] = {}

    def register(sid: str, path: str, seen: set[str]) -> None:
        if sid in seen or sid not in schemas_by_id:
            return
        seen.add(sid)
        _, keys, children = schemas_by_id[sid]
        by_path.setdefault(path, set()).update(keys)
        for child_name, child_sid in children:
            register(child_sid, normalize_path(f"{path}{child_name}/"), seen)

    for sid, (path, _, _) in schemas_by_id.items():
        if path:
            register(sid, path, set())

    return by_path


def parse_dconf(path: pathlib.Path) -> list[tuple[int, str, str]]:
    entries: list[tuple[int, str, str]] = []
    section: str | None = None
    for lineno, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#") or line.startswith(";"):
            continue
        if line.startswith("[") and line.endswith("]"):
            inner = line[1:-1].strip()
            section = normalize_path(inner) if inner else None
            continue
        if section is not None and "=" in line:
            key = line.split("=", 1)[0].strip()
            if key:
                entries.append((lineno, section, key))
    return entries


def main() -> int:
    repo_root = pathlib.Path(__file__).resolve().parent.parent
    schemas = load_schemas(repo_root)

    checked = 0
    checked_schemas: set[str] = set()
    skipped = 0
    errors: list[str] = []

    themes_dir = repo_root / "themes"
    for dconf_file in sorted(themes_dir.glob("*/*.dconf")):
        if dconf_file.parent.name == "macos-glass":
            continue
        rel = dconf_file.relative_to(repo_root)
        for lineno, section, key in parse_dconf(dconf_file):
            known_keys = schemas.get(section)
            if known_keys is None:
                skipped += 1
                continue
            checked += 1
            checked_schemas.add(section)
            if key not in known_keys:
                errors.append(f"{rel}:{lineno}: unknown key '{key}' in schema path '{section}'")

    for err in errors:
        print(f"error: {err}", file=sys.stderr)
    print(
        f"checked {checked} keys across {len(checked_schemas)} schemas, "
        f"{skipped} skipped (schema not installed)"
    )
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
