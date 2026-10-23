import Adw from 'gi://Adw';
import GLib from 'gi://GLib';
import Gtk from 'gi://Gtk';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

const SETTINGS_DIR = GLib.build_filenamev([GLib.get_user_config_dir(), 'grapefruit']);
const SETTINGS_PATH = GLib.build_filenamev([SETTINGS_DIR, 'settings.json']);

function loadSettings() {
    try {
        const [loaded, contents] = GLib.file_get_contents(SETTINGS_PATH);
        if (loaded) {
            const settings = JSON.parse(new TextDecoder().decode(contents));
            if (settings.convertModifier === 'control')
                return {convertModifier: 'control', actionsModifier: 'shift'};
        }
    } catch (_error) {
        // Missing or invalid settings use the documented defaults.
    }
    return {convertModifier: 'shift', actionsModifier: 'control'};
}

function saveSettings(convertModifier) {
    const settings = {
        convertModifier,
        actionsModifier: convertModifier === 'shift' ? 'control' : 'shift',
    };
    GLib.mkdir_with_parents(SETTINGS_DIR, 0o700);
    GLib.file_set_contents(SETTINGS_PATH, `${JSON.stringify(settings, null, 2)}\n`);
    return settings;
}

function modifierLabel(modifier) {
    return modifier === 'shift' ? 'Shift' : 'Ctrl';
}

export default class GrapefruitPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = loadSettings();
        const page = new Adw.PreferencesPage({
            title: 'Tuşlar',
            icon_name: 'preferences-desktop-keyboard-shortcuts-symbolic',
        });
        const group = new Adw.PreferencesGroup({
            title: 'Sürükleme menüsü',
            description: 'Bir dosyayı sürüklerken çemberi açan tuşları seçin.',
        });
        const convertRow = new Adw.ComboRow({
            title: 'Dönüştürme menüsü',
            subtitle: 'Çemberi bu tuşla açar.',
            model: Gtk.StringList.new(['Shift', 'Ctrl']),
            selected: settings.convertModifier === 'control' ? 1 : 0,
        });
        const actionsRow = new Adw.ActionRow({
            title: 'Dosya işlemleri menüsü',
            subtitle: 'Dönüştürme tuşunun diğeridir.',
        });
        const actionsLabel = new Gtk.Label({
            label: modifierLabel(settings.actionsModifier),
            valign: Gtk.Align.CENTER,
            css_classes: ['dim-label'],
        });
        actionsRow.add_suffix(actionsLabel);

        convertRow.connect('notify::selected', () => {
            const convertModifier = convertRow.selected === 1 ? 'control' : 'shift';
            const updated = saveSettings(convertModifier);
            actionsLabel.label = modifierLabel(updated.actionsModifier);
        });

        group.add(convertRow);
        group.add(actionsRow);
        page.add(group);
        window.add(page);
        window.set_default_size(540, 360);
    }
}
