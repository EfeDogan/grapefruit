#!/usr/bin/env bash
set -euo pipefail

integration_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
shell_uuid="grapefruit@io.github.efedogan"
shell_target="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$shell_uuid"
nautilus_target="${XDG_DATA_HOME:-$HOME/.local/share}/nautilus-python/extensions"
legacy_uuid="file-formatter@io.fileformatter.desktop"
legacy_shell_target="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$legacy_uuid"

gnome-extensions disable "$legacy_uuid" 2>/dev/null || true
rm -f -- "$nautilus_target/file_formatter.py"
rm -f -- "$legacy_shell_target/extension.js" "$legacy_shell_target/metadata.json"
rmdir -- "$legacy_shell_target" 2>/dev/null || true

install -d "$shell_target" "$nautilus_target"
install -m 0644 "$integration_root/gnome-shell/$shell_uuid/metadata.json" "$shell_target/metadata.json"
install -m 0644 "$integration_root/gnome-shell/$shell_uuid/extension.js" "$shell_target/extension.js"
install -m 0644 "$integration_root/nautilus/grapefruit.py" "$nautilus_target/grapefruit.py"

echo "Installed Grapefruit GNOME integration for the current user."
echo "Enable $shell_uuid after GNOME Shell discovers it, then restart GNOME Files."
