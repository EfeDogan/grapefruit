# Desktop integrations

The GNOME integration provides Tangerine-style modifier behavior without bypassing Wayland's input isolation:

- the Nautilus extension publishes the current local file selection to Grapefruit;
- the GNOME Shell extension watches for `Shift` or `Control` only while the primary pointer button is held during a drag from GNOME Files or the GNOME desktop;
- both components communicate with the desktop app over the user session D-Bus.

The first `Shift` opens the conversion wheel. While the drag continues, `Shift` switches to conversion and `Control` switches to file actions. Events are always propagated, so the integration does not swallow the original drag or keyboard input.

The mappings can be swapped from Grapefruit's **Ayarlar** dialog or the GNOME Extensions preferences button. Both interfaces write the same `~/.config/grapefruit/settings.json` file, which the Shell extension reloads while it is running. Windows uses the equivalent `%APPDATA%\\Grapefruit\\settings.json` file.

On GNOME/Wayland, the Shell extension also positions the wheel around the pointer location captured when the menu is opened. The position is clamped to the monitor work area so the full wheel remains visible near screen edges.

The scripts in `linux/` install or remove these files for the current user. Enabling a newly installed GNOME Shell extension and restarting GNOME Files are separate, user-visible steps.

Packages also contain a compatibility copy for the former `file-formatter@io.fileformatter.desktop` UUID. Grapefruit upgrades that user-installed extension in place and preserves whether it was enabled, preventing stale File Formatter integration files from shadowing the current extension.

The app also owns the former D-Bus name and object path during the transition. An extension module already cached by a running GNOME Shell therefore continues to open the current Grapefruit overlay until the next login loads the updated extension files.
