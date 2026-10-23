import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const BUS_NAME = 'io.github.efedogan.grapefruit.Integration';
const OBJECT_PATH = '/io/github/efedogan/grapefruit/Integration';
const INTERFACE_NAME = 'io.github.efedogan.grapefruit.Integration1';
const MIN_ACTIVATION_INTERVAL_US = 90_000;
const RELEASE_HIDE_DELAY_MS = 1_000;
const MODIFIER_POLL_INTERVAL_MS = 30;
const DRAG_THRESHOLD_PX = 8;
const OVERLAY_TITLE = 'Grapefruit Quick Menu';
const POSITION_RETRY_INTERVAL_MS = 25;
const POSITION_RETRY_LIMIT = 20;
const SETTINGS_RELOAD_INTERVAL_MS = 1_000;
const SETTINGS_PATH = GLib.build_filenamev([
    GLib.get_user_config_dir(),
    'grapefruit',
    'settings.json',
]);

export default class GrapefruitDragKeys extends Extension {
    enable() {
        this._lastActivationUs = 0;
        this._dragMenuActive = false;
        this._selectionSource = false;
        this._hideTimerId = 0;
        this._positionTimerId = 0;
        this._buttonDown = false;
        this._dragDistanceReached = false;
        this._buttonStartX = 0;
        this._buttonStartY = 0;
        this._wasConvertDown = false;
        this._wasActionsDown = false;
        this._convertModifier = 'shift';
        this._actionsModifier = 'control';
        this._loadModifierSettings();
        this._capturedEventId = global.stage.connect(
            'captured-event',
            (_actor, event) => this._onCapturedEvent(event),
        );
        this._modifierPollId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            MODIFIER_POLL_INTERVAL_MS,
            () => this._pollModifiers(),
        );
        this._settingsReloadId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            SETTINGS_RELOAD_INTERVAL_MS,
            () => {
                this._loadModifierSettings();
                return GLib.SOURCE_CONTINUE;
            },
        );
    }

    disable() {
        if (this._capturedEventId) {
            global.stage.disconnect(this._capturedEventId);
            this._capturedEventId = 0;
        }
        if (this._modifierPollId) {
            GLib.source_remove(this._modifierPollId);
            this._modifierPollId = 0;
        }
        if (this._settingsReloadId) {
            GLib.source_remove(this._settingsReloadId);
            this._settingsReloadId = 0;
        }
        this._cancelHideTimer();
        this._cancelPositionTimer();
        this._dragMenuActive = false;
        this._selectionSource = false;
    }

    _onCapturedEvent(event) {
        if (event.type() === Clutter.EventType.BUTTON_RELEASE) {
            if (this._dragMenuActive)
                this._finishDrag();
            return Clutter.EVENT_PROPAGATE;
        }

        if (event.type() !== Clutter.EventType.KEY_PRESS)
            return Clutter.EVENT_PROPAGATE;

        const state = event.get_state();
        if (!(state & Clutter.ModifierType.BUTTON1_MASK))
            return Clutter.EVENT_PROPAGATE;

        const key = event.get_key_symbol();
        let modifier = null;
        if (key === Clutter.KEY_Shift_L || key === Clutter.KEY_Shift_R)
            modifier = 'shift';
        else if (key === Clutter.KEY_Control_L || key === Clutter.KEY_Control_R)
            modifier = 'control';

        let mode = null;
        if (modifier === this._convertModifier)
            mode = 'convert';
        else if (modifier === this._actionsModifier)
            mode = 'actions';

        if (!mode)
            return Clutter.EVENT_PROPAGATE;

        if (!this._dragDistanceReached)
            return Clutter.EVENT_PROPAGATE;

        this._activateMode(mode);
        return Clutter.EVENT_PROPAGATE;
    }

    _pollModifiers() {
        const [x, y, state] = global.get_pointer();
        const buttonDown = Boolean(state & Clutter.ModifierType.BUTTON1_MASK);
        const shiftDown = Boolean(state & Clutter.ModifierType.SHIFT_MASK);
        const controlDown = Boolean(state & Clutter.ModifierType.CONTROL_MASK);
        const convertDown = this._modifierIsDown(
            this._convertModifier,
            shiftDown,
            controlDown,
        );
        const actionsDown = this._modifierIsDown(
            this._actionsModifier,
            shiftDown,
            controlDown,
        );

        if (!buttonDown) {
            if (this._dragMenuActive)
                this._finishDrag();
            this._buttonDown = false;
            this._dragDistanceReached = false;
            this._wasConvertDown = false;
            this._wasActionsDown = false;
            return GLib.SOURCE_CONTINUE;
        }

        if (!this._buttonDown) {
            this._buttonDown = true;
            this._buttonStartX = x;
            this._buttonStartY = y;
        }

        const crossedThreshold = !this._dragDistanceReached &&
            Math.hypot(x - this._buttonStartX, y - this._buttonStartY) >= DRAG_THRESHOLD_PX;
        if (crossedThreshold)
            this._dragDistanceReached = true;

        let mode = null;
        if (this._dragDistanceReached) {
            if (convertDown && !this._wasConvertDown)
                mode = 'convert';
            else if (actionsDown && !this._wasActionsDown)
                mode = 'actions';
            else if (crossedThreshold && convertDown)
                mode = 'convert';
            else if (crossedThreshold && actionsDown)
                mode = 'actions';
        }

        this._wasConvertDown = convertDown;
        this._wasActionsDown = actionsDown;
        if (mode)
            this._activateMode(mode);

        return GLib.SOURCE_CONTINUE;
    }

    _modifierIsDown(modifier, shiftDown, controlDown) {
        return modifier === 'control' ? controlDown : shiftDown;
    }

    _loadModifierSettings() {
        try {
            const [loaded, contents] = GLib.file_get_contents(SETTINGS_PATH);
            if (!loaded)
                return;
            const settings = JSON.parse(new TextDecoder().decode(contents));
            const convert = settings.convertModifier;
            const actions = settings.actionsModifier;
            const valid = ['shift', 'control'];
            if (valid.includes(convert) && valid.includes(actions) && convert !== actions) {
                this._convertModifier = convert;
                this._actionsModifier = actions;
            }
        } catch (_error) {
            // Missing or partially-written settings fall back to Shift / Control.
        }
    }

    _activateMode(mode) {
        if (!this._dragMenuActive) {
            const focusClass = global.display.focus_window?.get_wm_class()?.toLowerCase() ?? '';
            this._selectionSource = focusClass.includes('nautilus');
        }

        const now = GLib.get_monotonic_time();
        if (now - this._lastActivationUs < MIN_ACTIVATION_INTERVAL_US)
            return;
        this._lastActivationUs = now;

        this._cancelHideTimer();
        if (this._dragMenuActive) {
            this._callBooleanMethod('SwitchOverlayMode', new GLib.Variant('(s)', [mode]));
        } else {
            const [pointerX, pointerY] = global.get_pointer();
            this._dragMenuActive = true;
            if (this._selectionSource)
                this._showFromNautilus(mode, pointerX, pointerY);
            else
                this._callBooleanMethod(
                    'ShowOverlay',
                    new GLib.Variant('(s)', [mode]),
                    () => this._queueOverlayPosition(pointerX, pointerY),
                );
        }
    }

    _showFromNautilus(mode, pointerX, pointerY) {
        Gio.DBus.session.call(
            BUS_NAME,
            OBJECT_PATH,
            INTERFACE_NAME,
            'ShowForSelection',
            new GLib.Variant('(s)', [mode]),
            new GLib.VariantType('(b)'),
            Gio.DBusCallFlags.NONE,
            500,
            null,
            (connection, result) => {
                try {
                    const [opened] = connection.call_finish(result).deepUnpack();
                    if (opened) {
                        this._queueOverlayPosition(pointerX, pointerY);
                    } else {
                        this._showOverlayFallback(mode, pointerX, pointerY);
                    }
                } catch (_error) {
                    this._showOverlayFallback(mode, pointerX, pointerY);
                }
            },
        );
    }

    _showOverlayFallback(mode, pointerX, pointerY) {
        this._callBooleanMethod(
            'ShowOverlay',
            new GLib.Variant('(s)', [mode]),
            () => this._queueOverlayPosition(pointerX, pointerY),
        );
    }

    _callBooleanMethod(method, parameters, onSuccess = null) {
        Gio.DBus.session.call(
            BUS_NAME,
            OBJECT_PATH,
            INTERFACE_NAME,
            method,
            parameters,
            new GLib.VariantType('(b)'),
            Gio.DBusCallFlags.NONE,
            500,
            null,
            (connection, result) => {
                try {
                    const [succeeded] = connection.call_finish(result).deepUnpack();
                    if (succeeded)
                        onSuccess?.();
                } catch (_error) {
                    // The desktop app may not be running. Never consume the
                    // original modifier or drag event when integration fails.
                }
            },
        );
    }

    _queueOverlayPosition(pointerX, pointerY) {
        this._cancelPositionTimer();
        let attempts = 0;
        this._positionTimerId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            POSITION_RETRY_INTERVAL_MS,
            () => {
                attempts += 1;
                if (this._positionOverlayAt(pointerX, pointerY) ||
                    attempts >= POSITION_RETRY_LIMIT) {
                    this._positionTimerId = 0;
                    return GLib.SOURCE_REMOVE;
                }
                return GLib.SOURCE_CONTINUE;
            },
        );
    }

    _positionOverlayAt(pointerX, pointerY) {
        const overlay = global.display.list_all_windows().find(window =>
            window.get_title() === OVERLAY_TITLE,
        );
        if (!overlay)
            return false;

        const frame = overlay.get_frame_rect();
        if (frame.width <= 0 || frame.height <= 0)
            return false;

        const monitor = Main.layoutManager.monitors.find(candidate =>
            pointerX >= candidate.x && pointerX < candidate.x + candidate.width &&
            pointerY >= candidate.y && pointerY < candidate.y + candidate.height,
        ) ?? Main.layoutManager.primaryMonitor;
        if (!monitor)
            return false;

        const workArea = Main.layoutManager.getWorkAreaForMonitor(monitor.index);
        const maxX = Math.max(workArea.x, workArea.x + workArea.width - frame.width);
        const maxY = Math.max(workArea.y, workArea.y + workArea.height - frame.height);
        const targetX = Math.max(
            workArea.x,
            Math.min(Math.round(pointerX - frame.width / 2), maxX),
        );
        const targetY = Math.max(
            workArea.y,
            Math.min(Math.round(pointerY - frame.height / 2), maxY),
        );

        if (Math.abs(frame.x - targetX) <= 1 && Math.abs(frame.y - targetY) <= 1)
            return true;

        overlay.move_frame(true, targetX, targetY);
        return false;
    }

    _finishDrag() {
        this._dragMenuActive = false;
        this._selectionSource = false;
        this._cancelHideTimer();
        this._hideTimerId = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT,
            RELEASE_HIDE_DELAY_MS,
            () => {
                this._hideTimerId = 0;
                this._callBooleanMethod('HideOverlay', null);
                return GLib.SOURCE_REMOVE;
            },
        );
    }

    _cancelHideTimer() {
        if (!this._hideTimerId)
            return;
        GLib.source_remove(this._hideTimerId);
        this._hideTimerId = 0;
    }

    _cancelPositionTimer() {
        if (!this._positionTimerId)
            return;
        GLib.source_remove(this._positionTimerId);
        this._positionTimerId = 0;
    }
}
