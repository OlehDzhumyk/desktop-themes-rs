#!/bin/sh
# Синхронізує macos-glass з macos: символьні посилання на всі файли macos (крім тих, що тут свої),
# extensions.txt = список macos + розширення Liquid Glass. Запускати після змін у themes/macos.
set -eu
cd "$(dirname "$0")"
OWN="theme.conf glass.dconf extensions.txt README.md sync-glass.sh"
for f in ../macos/*; do
  n=$(basename "$f")
  case "$n" in *.bak*) continue;; esac
  case " $OWN " in *" $n "*) continue;; esac
  ln -sfn "../macos/$n" "$n"
done
# прибрати застарілі посилання (файл у macos зник)
for l in *; do
  [ -L "$l" ] && [ ! -e "$l" ] && unlink "$l"
done
{ cat ../macos/extensions.txt; echo liquid-glass@thinkingcoding1231.gmail.com; } | awk 'NF && !seen[$0]++' > extensions.txt.new
mv extensions.txt.new extensions.txt
