# Desktop integrations

The GNOME integration provides Tangerine-style modifier behavior without bypassing Wayland's input isolation:

- the Nautilus extension publishes the current local file selection to Grapefruit;
- the GNOME Shell extension watches for `Shift` or `Control` only while the primary pointer button is held during a drag from GNOME Files or the GNOME desktop;
- both components communicate with the desktop app over the user session D-Bus.

The first `Shift` opens the conversion wheel. While the drag continues, `Shift` switches to conversion and `Control` switches to file actions. Events are always propagated, so the integration does not swallow the original drag or keyboard input.

On GNOME/Wayland, the Shell extension also positions the wheel around the pointer location captured when the menu is opened. The position is clamped to the monitor work area so the full wheel remains visible near screen edges.

The scripts in `linux/` install or remove these files for the current user. Enabling a newly installed GNOME Shell extension and restarting GNOME Files are separate, user-visible steps.
