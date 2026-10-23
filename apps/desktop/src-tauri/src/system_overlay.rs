#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

const CONVERT_SHORTCUT: &str = "Ctrl+Shift+F";
const ACTIONS_SHORTCUT: &str = "Ctrl+Shift+A";
#[cfg(target_os = "linux")]
const APP_ID: &str = "io.github.efedogan.grapefruit";
#[cfg(target_os = "linux")]
const INTEGRATION_BUS_NAME: &str = "io.github.efedogan.grapefruit.Integration";
#[cfg(target_os = "linux")]
const INTEGRATION_OBJECT_PATH: &str = "/io/github/efedogan/grapefruit/Integration";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayMenuMode {
    Convert,
    Actions,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragKeyEvent {
    Activate {
        mode: OverlayMenuMode,
        initial: bool,
    },
    Release,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Debug, Default)]
struct DragKeyTracker {
    origin: Option<(i32, i32)>,
    dragging: bool,
    menu_active: bool,
    previous_shift: bool,
    previous_control: bool,
}

#[cfg(any(target_os = "windows", test))]
impl DragKeyTracker {
    const DRAG_THRESHOLD: i32 = 8;

    fn update(
        &mut self,
        left_pressed: bool,
        position: (i32, i32),
        shift_pressed: bool,
        control_pressed: bool,
    ) -> Option<DragKeyEvent> {
        if !left_pressed {
            let event = self.menu_active.then_some(DragKeyEvent::Release);
            self.origin = None;
            self.dragging = false;
            self.menu_active = false;
            self.previous_shift = shift_pressed;
            self.previous_control = control_pressed;
            return event;
        }

        let origin = *self.origin.get_or_insert(position);
        if !self.dragging
            && ((position.0 - origin.0).abs() >= Self::DRAG_THRESHOLD
                || (position.1 - origin.1).abs() >= Self::DRAG_THRESHOLD)
        {
            self.dragging = true;
        }

        let event = if self.dragging && !self.menu_active {
            let mode = if shift_pressed {
                Some(OverlayMenuMode::Convert)
            } else if control_pressed {
                Some(OverlayMenuMode::Actions)
            } else {
                None
            };
            mode.map(|mode| {
                self.menu_active = true;
                DragKeyEvent::Activate {
                    mode,
                    initial: true,
                }
            })
        } else if self.dragging && self.menu_active {
            if shift_pressed && !self.previous_shift {
                Some(DragKeyEvent::Activate {
                    mode: OverlayMenuMode::Convert,
                    initial: false,
                })
            } else if control_pressed && !self.previous_control {
                Some(DragKeyEvent::Activate {
                    mode: OverlayMenuMode::Actions,
                    initial: false,
                })
            } else {
                None
            }
        } else {
            None
        };

        self.previous_shift = shift_pressed;
        self.previous_control = control_pressed;
        event
    }
}

impl OverlayMenuMode {
    fn parse(value: &str) -> Self {
        if value.eq_ignore_ascii_case("actions") {
            Self::Actions
        } else {
            Self::Convert
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Convert => "convert",
            Self::Actions => "actions",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    backend: String,
    ready: bool,
    convert_shortcut: String,
    actions_shortcut: String,
    detail: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSelection {
    paths: Vec<String>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayPointer {
    x: f64,
    y: f64,
}

pub struct OverlayRuntime {
    mode: Mutex<Option<OverlayMenuMode>>,
    external_selection: Mutex<Vec<String>>,
    shortcut_status: Mutex<ShortcutStatus>,
}

impl Default for OverlayRuntime {
    fn default() -> Self {
        Self {
            mode: Mutex::new(None),
            external_selection: Mutex::new(Vec::new()),
            shortcut_status: Mutex::new(ShortcutStatus {
                backend: "initializing".to_owned(),
                ready: false,
                convert_shortcut: CONVERT_SHORTCUT.to_owned(),
                actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
                detail: "Global shortcuts are being initialized".to_owned(),
            }),
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct LinuxDesktopIntegration {
    app: AppHandle,
}

#[cfg(target_os = "linux")]
#[ashpd::zbus::interface(name = "io.github.efedogan.grapefruit.Integration1")]
impl LinuxDesktopIntegration {
    fn set_selection(&self, paths: Vec<String>) -> bool {
        let paths = local_existing_paths(paths);
        let has_selection = !paths.is_empty();
        if let Ok(mut current) = self.app.state::<OverlayRuntime>().external_selection.lock() {
            *current = paths;
        }
        has_selection
    }

    fn show_for_selection(&self, mode: &str) -> bool {
        let paths = self
            .app
            .state::<OverlayRuntime>()
            .external_selection
            .lock()
            .map(|paths| paths.clone())
            .unwrap_or_default();
        if paths.is_empty() {
            return false;
        }

        let mode = OverlayMenuMode::parse(mode);
        match present_overlay_with_selection(&self.app, mode, &paths) {
            Ok(()) => {
                eprintln!(
                    "[desktop-integration] opened {} menu for {} selected path(s)",
                    mode.as_str(),
                    paths.len()
                );
                true
            }
            Err(error) => {
                eprintln!("[desktop-integration] could not open overlay: {error}");
                false
            }
        }
    }

    fn show_overlay(&self, mode: &str) -> bool {
        let mode = OverlayMenuMode::parse(mode);
        match present_overlay(&self.app, mode) {
            Ok(()) => {
                eprintln!(
                    "[desktop-integration] opened {} menu without a preloaded selection",
                    mode.as_str()
                );
                true
            }
            Err(error) => {
                eprintln!("[desktop-integration] could not open overlay: {error}");
                false
            }
        }
    }

    fn switch_overlay_mode(&self, mode: &str) -> bool {
        let mode = OverlayMenuMode::parse(mode);
        match switch_overlay_mode_in_place(&self.app, mode) {
            Ok(()) => {
                eprintln!(
                    "[desktop-integration] switched open overlay to {} mode",
                    mode.as_str()
                );
                true
            }
            Err(error) => {
                eprintln!("[desktop-integration] could not switch overlay mode: {error}");
                false
            }
        }
    }

    fn hide_overlay(&self) -> bool {
        hide_overlay_window(&self.app).is_ok()
    }
}

#[cfg(target_os = "linux")]
fn local_existing_paths(paths: Vec<String>) -> Vec<String> {
    paths
        .into_iter()
        .filter_map(|path| {
            let path = PathBuf::from(path);
            path.exists().then(|| path.to_string_lossy().into_owned())
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub fn setup_linux_desktop_integration(app: &AppHandle) {
    let integration = LinuxDesktopIntegration { app: app.clone() };
    tauri::async_runtime::spawn(async move {
        let result = async {
            let connection = ashpd::zbus::connection::Builder::session()?
                .name(INTEGRATION_BUS_NAME)?
                .serve_at(INTEGRATION_OBJECT_PATH, integration)?
                .build()
                .await?;
            eprintln!(
                "[desktop-integration] ready on {}{}",
                INTEGRATION_BUS_NAME, INTEGRATION_OBJECT_PATH
            );
            std::future::pending::<()>().await;
            drop(connection);
            Ok::<(), ashpd::zbus::Error>(())
        }
        .await;

        if let Err(error) = result {
            eprintln!("[desktop-integration] setup failed: {error}");
        }
    });
}

pub fn setup_system_shortcuts(app: &AppHandle) {
    if std::env::var_os("FILE_FORMATTER_DISABLE_GLOBAL_SHORTCUTS").is_some() {
        set_status(
            app,
            ShortcutStatus {
                backend: "disabled".to_owned(),
                ready: false,
                convert_shortcut: CONVERT_SHORTCUT.to_owned(),
                actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
                detail: "Global shortcuts are disabled for this process".to_owned(),
            },
        );
        return;
    }

    #[cfg(target_os = "linux")]
    if is_wayland_session() {
        set_status(
            app,
            ShortcutStatus {
                backend: "xdg-portal".to_owned(),
                ready: false,
                convert_shortcut: CONVERT_SHORTCUT.to_owned(),
                actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
                detail: "Waiting for Wayland global shortcut permission".to_owned(),
            },
        );
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = run_wayland_shortcuts(app.clone()).await {
                set_status(
                    &app,
                    ShortcutStatus {
                        backend: "xdg-portal".to_owned(),
                        ready: false,
                        convert_shortcut: CONVERT_SHORTCUT.to_owned(),
                        actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
                        detail: format!("Wayland shortcut setup failed: {error}"),
                    },
                );
            }
        });
        return;
    }

    #[cfg(target_os = "windows")]
    let drag_keys_ready = match setup_windows_drag_keys(app) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("[windows-drag-keys] setup failed: {error}");
            false
        }
    };
    #[cfg(not(target_os = "windows"))]
    let drag_keys_ready = false;
    setup_native_shortcuts(app, drag_keys_ready);
}

#[cfg(target_os = "windows")]
fn setup_windows_drag_keys(app: &AppHandle) -> Result<(), String> {
    use device_query::{DeviceQuery, DeviceState, Keycode};
    use std::time::{Duration, Instant};

    let app = app.clone();
    std::thread::Builder::new()
        .name("file-formatter-drag-keys".to_owned())
        .spawn(move || {
            let device = DeviceState::new();
            let mut tracker = DragKeyTracker::default();
            let mut hide_after: Option<Instant> = None;
            let mut last_pointer: Option<(i32, i32)> = None;

            loop {
                let mouse = device.get_mouse();
                let keys = device.get_keys();
                let left_pressed = mouse.button_pressed.get(1).copied().unwrap_or(false);
                let shift_pressed =
                    keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift);
                let control_pressed =
                    keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl);

                if left_pressed {
                    hide_after = None;
                }

                match tracker.update(left_pressed, mouse.coords, shift_pressed, control_pressed) {
                    Some(DragKeyEvent::Activate {
                        mode,
                        initial: true,
                    }) => {
                        let selection = match crate::windows_selection::selected_paths() {
                            Ok(paths) => paths,
                            Err(error) => {
                                eprintln!(
                                    "[windows-drag-keys] could not read Explorer selection: {error}"
                                );
                                Vec::new()
                            }
                        };
                        let result = if selection.is_empty() {
                            present_overlay(&app, mode)
                        } else {
                            present_overlay_with_selection(&app, mode, &selection)
                        };
                        if let Err(error) = result {
                            eprintln!("[windows-drag-keys] could not open overlay: {error}");
                        }
                        last_pointer = None;
                    }
                    Some(DragKeyEvent::Activate {
                        mode,
                        initial: false,
                    }) => {
                        if let Err(error) = switch_overlay_mode_in_place(&app, mode) {
                            eprintln!("[windows-drag-keys] could not switch overlay: {error}");
                        }
                    }
                    Some(DragKeyEvent::Release) => {
                        let _ = emit_overlay_pointer(&app, "overlay-release", mouse.coords);
                        // The frontend normally closes itself after finishing the operation. This
                        // is a hard cleanup in case WebView event delivery fails unexpectedly.
                        hide_after = Some(Instant::now() + Duration::from_secs(5));
                        last_pointer = None;
                    }
                    None => {}
                }

                if tracker.menu_active && last_pointer != Some(mouse.coords) {
                    let _ = emit_overlay_pointer(&app, "overlay-pointer", mouse.coords);
                    last_pointer = Some(mouse.coords);
                }

                if hide_after.is_some_and(|deadline| Instant::now() >= deadline) {
                    let _ = hide_overlay_window(&app);
                    hide_after = None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "windows")]
fn emit_overlay_pointer(
    app: &AppHandle,
    event: &str,
    screen_position: (i32, i32),
) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;
    let origin = overlay
        .outer_position()
        .map_err(|error| error.to_string())?;
    overlay
        .emit(
            event,
            OverlayPointer {
                x: f64::from(screen_position.0 - origin.x),
                y: f64::from(screen_position.1 - origin.y),
            },
        )
        .map_err(|error| error.to_string())
}

fn setup_native_shortcuts(app: &AppHandle, drag_keys_ready: bool) {
    use tauri_plugin_global_shortcut::{
        Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState,
    };

    let convert = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF);
    let actions = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyA);
    let convert_for_handler = convert;
    let actions_for_handler = actions;

    let plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if shortcut == &convert_for_handler {
                let _ = toggle_overlay(app, OverlayMenuMode::Convert);
            } else if shortcut == &actions_for_handler {
                let _ = toggle_overlay(app, OverlayMenuMode::Actions);
            }
        })
        .build();

    let result = match app.plugin(plugin) {
        Ok(()) => app
            .global_shortcut()
            .register_multiple([convert, actions])
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };

    match result {
        Ok(()) => set_status(
            app,
            ShortcutStatus {
                backend: if cfg!(target_os = "windows") {
                    if drag_keys_ready {
                        "windows"
                    } else {
                        "windows-fallback"
                    }
                } else {
                    "x11"
                }
                .to_owned(),
                ready: true,
                convert_shortcut: if cfg!(target_os = "windows") && drag_keys_ready {
                    "Shift while dragging"
                } else {
                    CONVERT_SHORTCUT
                }
                .to_owned(),
                actions_shortcut: if cfg!(target_os = "windows") && drag_keys_ready {
                    "Control while dragging"
                } else {
                    ACTIONS_SHORTCUT
                }
                .to_owned(),
                detail: if cfg!(target_os = "windows") && drag_keys_ready {
                    "Windows drag keys and fallback global shortcuts are ready"
                } else if cfg!(target_os = "windows") {
                    "Fallback global shortcuts are ready; the drag-key monitor could not start"
                } else {
                    "System-wide shortcuts are ready"
                }
                .to_owned(),
            },
        ),
        Err(error) => {
            #[cfg(target_os = "windows")]
            set_status(
                app,
                ShortcutStatus {
                    backend: if drag_keys_ready {
                        "windows-drag-keys"
                    } else {
                        "windows-unavailable"
                    }
                    .to_owned(),
                    ready: drag_keys_ready,
                    convert_shortcut: if drag_keys_ready {
                        "Shift while dragging"
                    } else {
                        CONVERT_SHORTCUT
                    }
                    .to_owned(),
                    actions_shortcut: if drag_keys_ready {
                        "Control while dragging"
                    } else {
                        ACTIONS_SHORTCUT
                    }
                    .to_owned(),
                    detail: if drag_keys_ready {
                        format!(
                            "Drag shortcuts are ready; fallback global shortcuts are unavailable: {error}"
                        )
                    } else {
                        format!("Windows shortcuts are unavailable: {error}")
                    },
                },
            );
            #[cfg(not(target_os = "windows"))]
            set_status(
                app,
                ShortcutStatus {
                    backend: "native".to_owned(),
                    ready: false,
                    convert_shortcut: CONVERT_SHORTCUT.to_owned(),
                    actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
                    detail: format!("Global shortcut setup failed: {error}"),
                },
            );
        }
    }
}

#[cfg(target_os = "linux")]
async fn run_wayland_shortcuts(app: AppHandle) -> Result<(), String> {
    use ashpd::desktop::{
        CreateSessionOptions,
        global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
    };
    use ashpd::{AppID, register_host_app_with_connection};
    use futures_util::StreamExt;

    // Host applications do not have a sandbox manifest from which the portal can
    // infer an application ID. Register this process on the same D-Bus connection
    // used for GlobalShortcuts so GNOME can associate the permission with the app.
    let connection = ashpd::zbus::Connection::session()
        .await
        .map_err(|error| error.to_string())?;
    let app_id = APP_ID.parse::<AppID>().map_err(|error| error.to_string())?;
    register_host_app_with_connection(connection.clone(), app_id)
        .await
        .map_err(|error| error.to_string())?;
    let portal = GlobalShortcuts::with_connection(connection)
        .await
        .map_err(|error| error.to_string())?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    let mut activated = portal
        .receive_activated()
        .await
        .map_err(|error| error.to_string())?;
    let shortcuts = [
        NewShortcut::new("open-convert", "Grapefruit conversion menu")
            .preferred_trigger("CTRL+SHIFT+F"),
        NewShortcut::new("open-actions", "Grapefruit file actions menu")
            .preferred_trigger("CTRL+SHIFT+A"),
    ];
    let request = portal
        .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
        .await
        .map_err(|error| error.to_string())?;
    let response = request.response().map_err(|error| error.to_string())?;
    let bound = response
        .shortcuts()
        .iter()
        .map(|shortcut| shortcut.trigger_description())
        .collect::<Vec<_>>()
        .join(" / ");

    set_status(
        &app,
        ShortcutStatus {
            backend: format!("xdg-portal-v{}", portal.version()),
            ready: true,
            convert_shortcut: CONVERT_SHORTCUT.to_owned(),
            actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
            detail: if bound.is_empty() {
                "Wayland shortcuts are ready".to_owned()
            } else {
                format!("Wayland shortcuts are ready: {bound}")
            },
        },
    );

    while let Some(event) = activated.next().await {
        let mode = match event.shortcut_id() {
            "open-convert" => Some(OverlayMenuMode::Convert),
            "open-actions" => Some(OverlayMenuMode::Actions),
            _ => None,
        };
        if let Some(mode) = mode {
            let _ = toggle_overlay(&app, mode);
        }
    }

    Err("Wayland shortcut event stream ended".to_owned())
}

fn toggle_overlay(app: &AppHandle, mode: OverlayMenuMode) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;
    let current_mode = *app
        .state::<OverlayRuntime>()
        .mode
        .lock()
        .map_err(|_| "overlay state lock is poisoned")?;
    let is_visible = overlay.is_visible().map_err(|error| error.to_string())?;

    if is_visible && current_mode == Some(mode) {
        return hide_overlay_window(app);
    }
    present_overlay(app, mode)
}

fn present_overlay(app: &AppHandle, mode: OverlayMenuMode) -> Result<(), String> {
    if let Ok(mut selection) = app.state::<OverlayRuntime>().external_selection.lock() {
        selection.clear();
    }
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;
    overlay
        .emit("overlay-selection", ExternalSelection::default())
        .map_err(|error| error.to_string())?;
    present_overlay_mode(app, mode)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn present_overlay_with_selection(
    app: &AppHandle,
    mode: OverlayMenuMode,
    paths: &[String],
) -> Result<(), String> {
    if let Ok(mut selection) = app.state::<OverlayRuntime>().external_selection.lock() {
        *selection = paths.to_vec();
    }
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;

    overlay
        .emit(
            "overlay-selection",
            ExternalSelection {
                paths: paths.to_vec(),
            },
        )
        .map_err(|error| error.to_string())?;
    present_overlay_mode(app, mode)
}

fn present_overlay_mode(app: &AppHandle, mode: OverlayMenuMode) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;

    apply_overlay_mode(app, &overlay, mode)?;

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    position_overlay_near_cursor(&overlay);
    #[cfg(target_os = "linux")]
    if !is_wayland_session() {
        position_overlay_near_cursor(&overlay);
    }

    overlay.show().map_err(|error| error.to_string())
}

fn switch_overlay_mode_in_place(app: &AppHandle, mode: OverlayMenuMode) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;
    apply_overlay_mode(app, &overlay, mode)?;
    overlay.show().map_err(|error| error.to_string())
}

fn apply_overlay_mode(
    app: &AppHandle,
    overlay: &tauri::WebviewWindow,
    mode: OverlayMenuMode,
) -> Result<(), String> {
    *app.state::<OverlayRuntime>()
        .mode
        .lock()
        .map_err(|_| "overlay state lock is poisoned")? = Some(mode);
    overlay
        .emit("overlay-mode", mode.as_str())
        .map_err(|error| error.to_string())
}

fn position_overlay_near_cursor(overlay: &tauri::WebviewWindow) {
    let Ok(cursor) = overlay.cursor_position() else {
        return;
    };
    let Ok(size) = overlay.outer_size() else {
        return;
    };
    let Ok(monitors) = overlay.available_monitors() else {
        return;
    };
    let monitor = monitors.iter().find(|monitor| {
        let position = monitor.position();
        let monitor_size = monitor.size();
        cursor.x >= f64::from(position.x)
            && cursor.y >= f64::from(position.y)
            && cursor.x < f64::from(position.x) + f64::from(monitor_size.width)
            && cursor.y < f64::from(position.y) + f64::from(monitor_size.height)
    });

    let (mut x, mut y) = (
        cursor.x.round() as i32 - (size.width as i32 / 2),
        cursor.y.round() as i32 - (size.height as i32 / 2),
    );
    if let Some(monitor) = monitor {
        let position = monitor.position();
        let monitor_size = monitor.size();
        let max_x = position.x + monitor_size.width as i32 - size.width as i32;
        let max_y = position.y + monitor_size.height as i32 - size.height as i32;
        x = x.clamp(position.x, max_x.max(position.x));
        y = y.clamp(position.y, max_y.max(position.y));
    }
    let _ = overlay.set_position(PhysicalPosition::new(x, y));
}

fn hide_overlay_window(app: &AppHandle) -> Result<(), String> {
    let overlay = app
        .get_webview_window("overlay")
        .ok_or("overlay window is unavailable")?;
    overlay.hide().map_err(|error| error.to_string())?;
    *app.state::<OverlayRuntime>()
        .mode
        .lock()
        .map_err(|_| "overlay state lock is poisoned")? = None;
    Ok(())
}

fn set_status(app: &AppHandle, status: ShortcutStatus) {
    eprintln!(
        "[global-shortcuts] backend={} ready={} detail={}",
        status.backend, status.ready, status.detail
    );
    if let Ok(mut current) = app.state::<OverlayRuntime>().shortcut_status.lock() {
        *current = status.clone();
    }
    let _ = app.emit("global-shortcut-status", status);
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[tauri::command]
pub fn show_overlay(app: AppHandle, mode: String) -> Result<(), String> {
    present_overlay(&app, OverlayMenuMode::parse(&mode))
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) -> Result<(), String> {
    hide_overlay_window(&app)
}

#[tauri::command]
pub fn current_overlay_mode(state: State<'_, OverlayRuntime>) -> String {
    state
        .mode
        .lock()
        .ok()
        .and_then(|mode| *mode)
        .unwrap_or(OverlayMenuMode::Convert)
        .as_str()
        .to_owned()
}

#[tauri::command]
pub fn current_overlay_selection(state: State<'_, OverlayRuntime>) -> ExternalSelection {
    ExternalSelection {
        paths: state
            .external_selection
            .lock()
            .map(|paths| paths.clone())
            .unwrap_or_default(),
    }
}

#[tauri::command]
pub fn global_shortcut_status(state: State<'_, OverlayRuntime>) -> ShortcutStatus {
    state
        .shortcut_status
        .lock()
        .map(|status| status.clone())
        .unwrap_or_else(|_| ShortcutStatus {
            backend: "unknown".to_owned(),
            ready: false,
            convert_shortcut: CONVERT_SHORTCUT.to_owned(),
            actions_shortcut: ACTIONS_SHORTCUT.to_owned(),
            detail: "Shortcut state is unavailable".to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_below_drag_threshold_does_not_activate() {
        let mut tracker = DragKeyTracker::default();
        assert_eq!(tracker.update(true, (100, 100), false, false), None);
        assert_eq!(tracker.update(true, (107, 100), true, false), None);
    }

    #[test]
    fn shift_opens_convert_after_drag_threshold() {
        let mut tracker = DragKeyTracker::default();
        tracker.update(true, (100, 100), false, false);
        assert_eq!(
            tracker.update(true, (108, 100), true, false),
            Some(DragKeyEvent::Activate {
                mode: OverlayMenuMode::Convert,
                initial: true,
            })
        );
    }

    #[test]
    fn held_modifier_activates_when_drag_threshold_is_crossed() {
        let mut tracker = DragKeyTracker::default();
        tracker.update(true, (100, 100), true, false);
        assert_eq!(
            tracker.update(true, (100, 108), true, false),
            Some(DragKeyEvent::Activate {
                mode: OverlayMenuMode::Convert,
                initial: true,
            })
        );
    }

    #[test]
    fn control_switches_an_open_menu_without_reopening_it() {
        let mut tracker = DragKeyTracker::default();
        tracker.update(true, (100, 100), false, false);
        tracker.update(true, (108, 100), true, false);
        tracker.update(true, (110, 100), false, false);
        assert_eq!(
            tracker.update(true, (112, 100), false, true),
            Some(DragKeyEvent::Activate {
                mode: OverlayMenuMode::Actions,
                initial: false,
            })
        );
    }

    #[test]
    fn releasing_mouse_ends_an_active_drag_menu() {
        let mut tracker = DragKeyTracker::default();
        tracker.update(true, (100, 100), false, false);
        tracker.update(true, (108, 100), true, false);
        assert_eq!(
            tracker.update(false, (108, 100), false, false),
            Some(DragKeyEvent::Release)
        );
        assert_eq!(tracker.update(false, (108, 100), false, false), None);
    }
}
