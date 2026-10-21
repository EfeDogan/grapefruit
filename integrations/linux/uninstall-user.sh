#!/usr/bin/env bash
set -euo pipefail

shell_uuid="grapefruit@io.github.efedogan"
legacy_uuid="file-formatter@io.fileformatter.desktop"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
shell_target="$data_home/gnome-shell/extensions/$shell_uuid"
nautilus_target="$data_home/nautilus-python/extensions/grapefruit.py"
legacy_shell_target="$data_home/gnome-shell/extensions/$legacy_uuid"

gnome-extensions disable "$shell_uuid" 2>/dev/null || true
gnome-extensions disable "$legacy_uuid" 2>/dev/null || true
rm -f -- "$nautilus_target" "$data_home/nautilus-python/extensions/file_formatter.py"
rm -f -- "$shell_target/extension.js" "$shell_target/metadata.json"
rm -f -- "$legacy_shell_target/extension.js" "$legacy_shell_target/metadata.json"
rmdir -- "$shell_target" 2>/dev/null || true
rmdir -- "$legacy_shell_target" 2>/dev/null || true

echo "Removed Grapefruit GNOME integration for the current user."
