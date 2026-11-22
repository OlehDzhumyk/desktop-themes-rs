#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Prepare a release: set the version everywhere and roll the changelog. It edits files only; it does
# not commit, tag or push (the commands to do that are printed).
#
#   scripts/release.sh [--dry-run] [--allow-empty] [--date YYYY-MM-DD] VERSION     e.g. 0.3.0 or 1.0.0-rc.1
#
# Updates:
#   Cargo.toml                [workspace.package] version, and the workspace crates (crates/*, xtask) in Cargo.lock
#   bridge/package.json       version (and the root entries of package-lock.json)
#   bridge/static/metadata.json  "version-name" = VERSION, "version" (the integer GNOME wants) + 1
#   CHANGELOG.md              "## [Unreleased]" becomes "## [VERSION] - DATE", a new empty Unreleased
#                             section is added on top, the compare links at the bottom are rewritten
# Then, once the diff looks right:
#   git commit -am "Release VERSION" && git tag -s vVERSION -m "VERSION" && git push origin main vVERSION
# Pushing the tag starts .github/workflows/release.yml, which drafts the GitHub release.
set -euo pipefail
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DRY=0 EMPTY=0 DATE=$(date -u +%F) VERSION=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run|-n) DRY=1; shift ;;
    --allow-empty) EMPTY=1; shift ;;
    --date) DATE=$2; shift 2 ;;
    -h|--help) sed -n '2,19p' "$0"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) [ -z "$VERSION" ] || { echo "one VERSION only" >&2; exit 2; }; VERSION=$1; shift ;;
  esac
done
[ -n "$VERSION" ] || { sed -n '2,19p' "$0" >&2; exit 2; }
VERSION=${VERSION#v}
printf '%s' "$VERSION" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$' \
  || { echo "error: '$VERSION' is not a semantic version (MAJOR.MINOR.PATCH[-prerelease])" >&2; exit 2; }
printf '%s' "$DATE" | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}$' || { echo "error: bad --date" >&2; exit 2; }
cd "$REPO"
if [ "$DRY" = 0 ] && [ -n "$(git status --porcelain --untracked-files=no 2>/dev/null)" ]; then
  echo "error: the working tree has uncommitted changes; commit or stash them first" >&2
  exit 1
fi
if git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null 2>&1; then
  echo "error: tag v$VERSION already exists" >&2
  exit 1
fi

DRY=$DRY EMPTY=$EMPTY python3 - "$VERSION" "$DATE" <<'PY'
import json, os, re, sys

version, date = sys.argv[1:3]
dry, allow_empty = os.environ["DRY"] == "1", os.environ["EMPTY"] == "1"
changed = []


def write(path, new, old):
    if new == old:
        return
    changed.append(path)
    if not dry:
        with open(path, "w", encoding="utf-8") as f:
            f.write(new)


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


# Cargo.toml: the version line of [workspace.package]
s = read("Cargo.toml")
m = re.search(r"(\[workspace\.package\][^\[]*?^version = \")([^\"]+)(\")", s, re.M | re.S)
if not m:
    sys.exit("error: [workspace.package] version not found in Cargo.toml")
old_version = m.group(2)
write("Cargo.toml", s[:m.start(2)] + version + s[m.end(2):], s)

# Cargo.lock: workspace members only (path crates have no source line)
members = set()
for toml in [os.path.join("crates", c, "Cargo.toml") for c in os.listdir("crates")] + ["xtask/Cargo.toml"]:
    if os.path.exists(toml):
        mm = re.search(r'^name = "([^"]+)"', read(toml), re.M)
        if mm and "version.workspace = true" in read(toml):
            members.add(mm.group(1))
lock = read("Cargo.lock")
blocks = lock.split("\n[[package]]\n")


def bump(block):
    m = re.match(r'name = "([^"]+)"\nversion = "[^"]+"\n', block)
    if m and m.group(1) in members and "\nsource = " not in "\n" + block:
        return 'name = "%s"\nversion = "%s"\n' % (m.group(1), version) + block[m.end():]
    return block


write("Cargo.lock", "\n[[package]]\n".join([blocks[0]] + [bump(x) for x in blocks[1:]]), lock)

# bridge npm files
for path in ("bridge/package.json", "bridge/package-lock.json"):
    old = read(path)
    data = json.loads(old)
    data["version"] = version
    if "packages" in data and "" in data["packages"]:
        data["packages"][""]["version"] = version
    write(path, json.dumps(data, indent=2, ensure_ascii=False) + "\n", old)

# extension metadata
path = "bridge/static/metadata.json"
old = read(path)
vm = re.search(r'("version"\s*:\s*)(\d+)', old)
if not vm:
    sys.exit("error: integer \"version\" not found in " + path)
txt = old[:vm.start(2)] + str(int(vm.group(2)) + 1) + old[vm.end(2):]
if re.search(r'"version-name"\s*:', txt):
    txt = re.sub(r'("version-name"\s*:\s*)"[^"]*"', lambda m: m.group(1) + '"' + version + '"', txt)
else:  # keep the file's own formatting: put it right after "version"
    txt = re.sub(r'("version"\s*:\s*\d+)(,?)', lambda m: m.group(1) + ',\n  "version-name": "' + version + '"' + m.group(2), txt, count=1)
write(path, txt, old)

# CHANGELOG
path = "CHANGELOG.md"
old = read(path)
m = re.search(r"^## \[Unreleased\]\n(.*?)(?=^## \[)", old, re.M | re.S)
if not m:
    sys.exit("error: CHANGELOG.md has no '## [Unreleased]' section followed by a release")
if not re.search(r"^- ", m.group(1), re.M) and not allow_empty:
    sys.exit("error: the Unreleased section of CHANGELOG.md has no entries (use --allow-empty to release anyway)")
if re.search(r"^## \[%s\]" % re.escape(version), old, re.M):
    sys.exit("error: CHANGELOG.md already has a section for %s" % version)
new = old[:m.start()] + "## [Unreleased]\n\n## [%s] - %s\n" % (version, date) + m.group(1) + old[m.end():]
lm = re.search(r"^\[Unreleased\]: (\S+)/compare/(\S+)\.\.\.HEAD$", new, re.M)
if not lm:
    sys.exit("error: the [Unreleased] compare link is missing at the bottom of CHANGELOG.md")
base, prev = lm.group(1), lm.group(2)
links = "[Unreleased]: %s/compare/v%s...HEAD\n[%s]: %s/compare/%s...v%s" % (base, version, version, base, prev, version)
new = new[:lm.start()] + links + new[lm.end():]
write(path, new, old)

print("%s version %s -> %s%s" % ("would set" if dry else "set", old_version, version, " (dry run)" if dry else ""))
for p in changed:
    print("  " + ("would update " if dry else "updated ") + p)
PY

if [ "$DRY" = 0 ]; then
  if command -v cargo >/dev/null 2>&1; then
    cargo metadata --locked --offline --format-version 1 >/dev/null || {
      echo "error: Cargo.lock does not match the new version; check the diff" >&2; exit 1; }
  else
    echo "note: cargo not found, Cargo.lock consistency not checked (run: cargo metadata --locked --offline)"
  fi
  git --no-pager diff --stat
  cat <<MSG

Review the diff, then:
  git commit -am "Release $VERSION"
  git tag -s v$VERSION -m "$VERSION"      # or -a if you do not sign
  git push origin main v$VERSION          # the tag starts .github/workflows/release.yml (draft release)
MSG
fi
