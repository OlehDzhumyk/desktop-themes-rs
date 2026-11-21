#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Стани розширень (через org.gnome.Shell.Extensions) — запускати через run.sh
for u in $(dconf read /org/gnome/shell/enabled-extensions | tr -d "[]',"); do
  s=$(gdbus call --session -d org.gnome.Shell -o /org/gnome/Shell -m org.gnome.Shell.Extensions.GetExtensionInfo "$u" | grep -o "'state': <[0-9.]*>" | grep -o "[0-9.]*>" | tr -d '>')
  case "${s%.*}" in 1) n=ACTIVE;; 2) n=INACTIVE;; 3) n=ERROR;; 4) n=OUT_OF_DATE;; 6) n=INITIALIZED;; "") n=UNKNOWN;; *) n="STATE_$s";; esac
  printf '  %-55s %s\n' "$u" "$n"
done
