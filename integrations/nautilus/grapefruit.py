from gi.repository import Gio, GLib, GObject, Nautilus


BUS_NAME = "io.github.efedogan.grapefruit.Integration"
OBJECT_PATH = "/io/github/efedogan/grapefruit/Integration"
INTERFACE_NAME = "io.github.efedogan.grapefruit.Integration1"


class GrapefruitSelectionProvider(GObject.GObject, Nautilus.MenuProvider):
    """Publishes GNOME Files' current local selection to Grapefruit."""

    def __init__(self):
        super().__init__()
        self._connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)

    def get_file_items(self, files):
        paths = []
        for file_info in files:
            location = file_info.get_location()
            path = location.get_path() if location else None
            if path:
                paths.append(path)

        self._publish_selection(paths)
        return []

    def get_background_items(self, _current_folder):
        return []

    def _publish_selection(self, paths):
        self._connection.call(
            BUS_NAME,
            OBJECT_PATH,
            INTERFACE_NAME,
            "SetSelection",
            GLib.Variant("(as)", (paths,)),
            GLib.VariantType.new("(b)"),
            Gio.DBusCallFlags.NONE,
            300,
            None,
            self._on_publish_finished,
            None,
        )

    @staticmethod
    def _on_publish_finished(connection, result, _user_data):
        try:
            connection.call_finish(result)
        except GLib.Error:
            # Grapefruit can be closed; selection tracking must never make
            # GNOME Files display an error or block its menus.
            pass
