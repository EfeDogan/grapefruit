use std::sync::Mutex;
#[cfg(target_os = "linux")]
use std::{fs, path::PathBuf, process::Command};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};

use crate::modifier_settings::{self as modifier_config, ModifierSettings};

#[cfg(target_os = "linux")]
const INTEGRATION_BUS_NAME: &str = "io.github.efedogan.grapefruit.Integration";
#[cfg(target_os = "linux")]
const INTEGRATION_OBJECT_PATH: &str = "/io/github/efedogan/grapefruit/Integration";
#[cfg(target_os = "linux")]
const LEGACY_INTEGRATION_BUS_NAME: &str = "io.fileformatter.desktop.Integration";
#[cfg(target_os = "linux")]
const LEGACY_INTEGRATION_OBJECT_PATH: &str = "/io/fileformatter/desktop/Integration";

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
    activation_rejected: bool,
    previous_convert: bool,
    previous_actions: bool,
}

#[cfg(any(target_os = "windows", test))]
impl DragKeyTracker {
    const DRAG_THRESHOLD: i32 = 8;

    fn update(
        &mut self,
        left_pressed: bool,
        position: (i32, i32),
        convert_pressed: bool,
        actions_pressed: bool,
    ) -> Option<DragKeyEvent> {
        if !left_pressed {
            let event = self.menu_active.then_some(DragKeyEvent::Release);
            self.origin = None;
            self.dragging = false;
            self.menu_active = false;
            self.activation_rejected = false;
            self.previous_convert = convert_pressed;
            self.previous_actions = actions_pressed;
            return event;
        }

        let origin = *self.origin.get_or_insert(position);
        if !self.dragging
            && ((position.0 - origin.0).abs() >= Self::DRAG_THRESHOLD
                || (position.1 - origin.1).abs() >= Self::DRAG_THRESHOLD)
        {
            self.dragging = true;
        }

        let event = if self.dragging && !self.menu_active && !self.activation_rejected {
            let mode = if convert_pressed {
                Some(OverlayMenuMode::Convert)
            } else if actions_pressed {
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
            if convert_pressed && !self.previous_convert {
                Some(DragKeyEvent::Activate {
                    mode: OverlayMenuMode::Convert,
                    initial: false,
                })
            } else if actions_pressed && !self.previous_actions {
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

        self.previous_convert = convert_pressed;
        self.previous_actions = actions_pressed;
        event
    }

    #[cfg(any(target_os = "windows", test))]
    fn reject_activation(&mut self) {
        self.menu_active = false;
        self.activation_rejected = true;
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
pub struct DragIntegrationStatus {
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
    modifier_settings: Mutex<ModifierSettings>,
    drag_integration_status: Mutex<DragIntegrationStatus>,
}

impl Default for OverlayRuntime {
    fn default() -> Self {
        let modifier_settings = modifier_config::load();
        Self {
            mode: Mutex::new(None),
            external_selection: Mutex::new(Vec::new()),
            modifier_settings: Mutex::new(modifier_settings),
            drag_integration_status: Mutex::new(DragIntegrationStatus {
                backend: "initializing".to_owned(),
                ready: false,
                convert_shortcut: drag_key_label(modifier_settings.convert_modifier),
                actions_shortcut: drag_key_label(modifier_settings.actions_modifier),
                detail: "File-drag integration is being initialized".to_owned(),
            }),
        }
    }
}

fn drag_key_label(key: crate::modifier_settings::ModifierKey) -> String {
    format!("{} while dragging", key.label())
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct LinuxDesktopIntegration {
    app: AppHandle,
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct LegacyLinuxDesktopIntegration {
    current: LinuxDesktopIntegration,
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
#[ashpd::zbus::interface(name = "io.fileformatter.desktop.Integration1")]
impl LegacyLinuxDesktopIntegration {
    fn set_selection(&self, paths: Vec<String>) -> bool {
        self.current.set_selection(paths)
    }

    fn show_for_selection(&self, mode: &str) -> bool {
        let opened = self.current.show_for_selection(mode);
        if opened {
            set_overlay_title(&self.current.app, "File Formatter Quick Menu");
        }
        opened
    }

    fn show_overlay(&self, mode: &str) -> bool {
        let _ = mode;
        // A GNOME Shell session can keep the pre-Grapefruit extension module in memory until the
        // next login. That module called ShowOverlay for every mouse drag, including text
        // selection in browsers and editors. Refuse that ambiguous legacy call; Nautilus still
        // uses ShowForSelection, while the updated extension calls the current interface only
        // after confirming the source is Files or the desktop.
        false
    }

    fn switch_overlay_mode(&self, mode: &str) -> bool {
        self.current.switch_overlay_mode(mode)
    }

    fn hide_overlay(&self) -> bool {
        self.current.hide_overlay()
    }
}

#[cfg(target_os = "linux")]
fn set_overlay_title(app: &AppHandle, title: &str) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.set_title(title);
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
    if let Err(error) = upgrade_legacy_gnome_extension() {
        eprintln!("[desktop-integration] legacy GNOME extension upgrade failed: {error}");
    }
    let integration = LinuxDesktopIntegration { app: app.clone() };
    let legacy_integration = LegacyLinuxDesktopIntegration {
        current: integration.clone(),
    };
    tauri::async_runtime::spawn(async move {
        let result = async {
            let connection = ashpd::zbus::connection::Builder::session()?
                .name(INTEGRATION_BUS_NAME)?
                .serve_at(INTEGRATION_OBJECT_PATH, integration)?
                .serve_at(LEGACY_INTEGRATION_OBJECT_PATH, legacy_integration)?
                .build()
                .await?;
            connection.request_name(LEGACY_INTEGRATION_BUS_NAME).await?;
            eprintln!(
                "[desktop-integration] ready on {}{} and legacy {}{}",
                INTEGRATION_BUS_NAME,
                INTEGRATION_OBJECT_PATH,
                LEGACY_INTEGRATION_BUS_NAME,
                LEGACY_INTEGRATION_OBJECT_PATH
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

#[cfg(target_os = "linux")]
fn upgrade_legacy_gnome_extension() -> Result<(), String> {
    const UUID: &str = "file-formatter@io.fileformatter.desktop";
    const FILES: [&str; 3] = ["extension.js", "prefs.js", "metadata.json"];

    let Some(home) = std::env::var_os("HOME") else {
        return Ok(());
    };
    let source = PathBuf::from("/usr/share/gnome-shell/extensions").join(UUID);
    let destination = PathBuf::from(home)
        .join(".local/share/gnome-shell/extensions")
        .join(UUID);
    if !source.is_dir() || !destination.is_dir() {
        return Ok(());
    }

    let changed = FILES.iter().try_fold(false, |changed, file| {
        let packaged = fs::read(source.join(file))
            .map_err(|error| format!("{file} paketten okunamadı: {error}"))?;
        let installed = fs::read(destination.join(file)).unwrap_or_default();
        Ok::<bool, String>(changed || packaged != installed)
    })?;
    if !changed {
        return Ok(());
    }

    let was_active = Command::new("gnome-extensions")
        .args(["info", UUID])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.trim().eq_ignore_ascii_case("State: ACTIVE"))
        });
    fs::create_dir_all(&destination)
        .map_err(|error| format!("eski eklenti dizini hazırlanamadı: {error}"))?;
    for file in FILES {
        fs::copy(source.join(file), destination.join(file))
            .map_err(|error| format!("{file} yükseltilemedi: {error}"))?;
    }

    if was_active {
        let _ = Command::new("gnome-extensions")
            .args(["disable", UUID])
            .status();
        let status = Command::new("gnome-extensions")
            .args(["enable", UUID])
            .status()
            .map_err(|error| format!("GNOME eklentisi yeniden etkinleştirilemedi: {error}"))?;
        if !status.success() {
            return Err(format!(
                "GNOME eklentisi yeniden etkinleştirilemedi (çıkış kodu: {status})"
            ));
        }
    }
    eprintln!("[desktop-integration] upgraded legacy GNOME extension {UUID}");
    Ok(())
}

pub fn setup_drag_integration(app: &AppHandle) {
    let settings = app
        .state::<OverlayRuntime>()
        .modifier_settings
        .lock()
        .map(|settings| *settings)
        .unwrap_or_default();
    let convert_shortcut = drag_key_label(settings.convert_modifier);
    let actions_shortcut = drag_key_label(settings.actions_modifier);

    #[cfg(target_os = "windows")]
    {
        let (ready, backend, detail) = match setup_windows_drag_keys(app) {
            Ok(()) => (
                true,
                "windows-drag-keys",
                "Windows Explorer and desktop drag keys are ready".to_owned(),
            ),
            Err(error) => {
                eprintln!("[windows-drag-keys] setup failed: {error}");
                (
                    false,
                    "windows-unavailable",
                    format!("Windows drag-key monitor could not start: {error}"),
                )
            }
        };
        set_status(
            app,
            DragIntegrationStatus {
                backend: backend.to_owned(),
                ready,
                convert_shortcut,
                actions_shortcut,
                detail,
            },
        );
    }

    #[cfg(target_os = "linux")]
    {
        set_status(
            app,
            DragIntegrationStatus {
                backend: "linux-file-manager-integration".to_owned(),
                ready: true,
                convert_shortcut,
                actions_shortcut,
                detail: "GNOME Files and desktop drag integration is ready".to_owned(),
            },
        );
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        set_status(
            app,
            DragIntegrationStatus {
                backend: "unsupported".to_owned(),
                ready: false,
                convert_shortcut,
                actions_shortcut,
                detail: "File-drag integration is unavailable on this platform".to_owned(),
            },
        );
    }
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
            let mut drag_selection = Vec::new();

            loop {
                let mouse = device.get_mouse();
                let keys = device.get_keys();
                let left_pressed = mouse.button_pressed.get(1).copied().unwrap_or(false);
                let gesture_started = left_pressed && tracker.origin.is_none();
                let shift_pressed =
                    keys.contains(&Keycode::LShift) || keys.contains(&Keycode::RShift);
                let control_pressed =
                    keys.contains(&Keycode::LControl) || keys.contains(&Keycode::RControl);
                let modifier_settings = app
                    .state::<OverlayRuntime>()
                    .modifier_settings
                    .lock()
                    .map(|settings| *settings)
                    .unwrap_or_default();
                let convert_pressed = modifier_settings
                    .convert_modifier
                    .is_pressed(shift_pressed, control_pressed);
                let actions_pressed = modifier_settings
                    .actions_modifier
                    .is_pressed(shift_pressed, control_pressed);

                if left_pressed {
                    hide_after = None;
                } else {
                    drag_selection.clear();
                }

                // Capture the Shell selection on the first observed mouse-down. On the Windows
                // desktop, the drag-image window can cover the icon by the time the pointer has
                // crossed our drag threshold, so WindowFromPoint can no longer identify WorkerW.
                // Explorer windows happened to work because their foreground frame remained a
                // usable fallback; the desktop has no equivalent guarantee.
                if gesture_started {
                    drag_selection = match crate::windows_selection::selected_paths_at(mouse.coords)
                    {
                        Ok(paths) => paths,
                        Err(error) => {
                            eprintln!(
                                "[windows-drag-keys] could not read source selection on mouse-down: {error}"
                            );
                            Vec::new()
                        }
                    };
                }

                let was_dragging = tracker.dragging;
                let drag_event =
                    tracker.update(left_pressed, mouse.coords, convert_pressed, actions_pressed);

                // Snapshot the Shell selection as soon as the pointer crosses the drag threshold,
                // while it is still over the source Explorer/desktop view. This keeps the source
                // available if the user moves over another window before pressing Shift or Ctrl,
                // without ever probing a browser/editor drag later in the gesture.
                if tracker.dragging && !was_dragging && drag_selection.is_empty() {
                    let source_position = tracker.origin.unwrap_or(mouse.coords);
                    drag_selection = match crate::windows_selection::selected_paths_at(source_position) {
                        Ok(paths) => paths,
                        Err(error) => {
                            eprintln!(
                                "[windows-drag-keys] could not read Explorer selection: {error}"
                            );
                            Vec::new()
                        }
                    };
                }

                match drag_event {
                    Some(DragKeyEvent::Activate {
                        mode,
                        initial: true,
                    }) => {
                        if drag_selection.is_empty() {
                            tracker.reject_activation();
                            eprintln!(
                                "[windows-drag-keys] ignored drag outside Explorer or without a file selection"
                            );
                        } else if let Err(error) =
                            present_overlay_with_selection(&app, mode, &drag_selection)
                        {
                            tracker.reject_activation();
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

fn set_status(app: &AppHandle, status: DragIntegrationStatus) {
    eprintln!(
        "[drag-integration] backend={} ready={} detail={}",
        status.backend, status.ready, status.detail
    );
    if let Ok(mut current) = app.state::<OverlayRuntime>().drag_integration_status.lock() {
        *current = status.clone();
    }
    let _ = app.emit("drag-integration-status", status);
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
pub fn drag_integration_status(state: State<'_, OverlayRuntime>) -> DragIntegrationStatus {
    state
        .drag_integration_status
        .lock()
        .map(|status| status.clone())
        .unwrap_or_else(|_| DragIntegrationStatus {
            backend: "unknown".to_owned(),
            ready: false,
            convert_shortcut: "Shift while dragging".to_owned(),
            actions_shortcut: "Ctrl while dragging".to_owned(),
            detail: "File-drag integration state is unavailable".to_owned(),
        })
}

#[tauri::command]
pub fn get_modifier_settings(state: State<'_, OverlayRuntime>) -> ModifierSettings {
    state
        .modifier_settings
        .lock()
        .map(|settings| *settings)
        .unwrap_or_default()
}

#[tauri::command]
pub fn set_modifier_settings(
    app: AppHandle,
    state: State<'_, OverlayRuntime>,
    convert_modifier: String,
) -> Result<ModifierSettings, String> {
    let settings = ModifierSettings::from_convert_modifier(&convert_modifier)?;
    modifier_config::save(settings)?;
    *state
        .modifier_settings
        .lock()
        .map_err(|_| "Tuş ayarı kilidi kullanılamıyor".to_owned())? = settings;
    app.emit("modifier-settings", settings)
        .map_err(|error| error.to_string())?;
    Ok(settings)
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

    #[test]
    fn rejected_non_file_drag_does_not_retry_until_mouse_release() {
        let mut tracker = DragKeyTracker::default();
        tracker.update(true, (100, 100), false, false);
        assert!(tracker.update(true, (108, 100), true, false).is_some());
        tracker.reject_activation();
        assert_eq!(tracker.update(true, (116, 100), true, false), None);
        assert_eq!(tracker.update(true, (124, 100), false, true), None);

        tracker.update(false, (124, 100), false, false);
        tracker.update(true, (200, 200), false, false);
        assert!(tracker.update(true, (208, 200), true, false).is_some());
    }
}
