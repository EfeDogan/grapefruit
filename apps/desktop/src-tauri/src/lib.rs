use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use file_formatter_core::{
    ConversionEngine, FileKind, TargetFormat, TransferOperation, execute_transfer, file_kind,
    plan_conversion, plan_transfer, supported_targets,
};
use serde::{Deserialize, Serialize};
use tauri::Manager as _;

mod system_overlay;

use system_overlay::{
    OverlayRuntime, current_overlay_mode, current_overlay_selection, global_shortcut_status,
    hide_overlay, show_overlay,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileCapability {
    path: String,
    name: String,
    kind: &'static str,
    targets: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeResponse {
    files: Vec<FileCapability>,
    common_targets: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ConversionResponse {
    outputs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RequestedTransferOperation {
    Copy,
    Move,
}

impl From<RequestedTransferOperation> for TransferOperation {
    fn from(operation: RequestedTransferOperation) -> Self {
        match operation {
            RequestedTransferOperation::Copy => Self::Copy,
            RequestedTransferOperation::Move => Self::Move,
        }
    }
}

#[derive(Debug, Serialize)]
struct TransferResponse {
    outputs: Vec<String>,
}

#[tauri::command]
fn analyze_files(paths: Vec<String>) -> AnalyzeResponse {
    let files = paths
        .into_iter()
        .map(|path| {
            let file_path = PathBuf::from(&path);
            let targets = supported_targets(&file_path)
                .into_iter()
                .map(|target| target.to_string())
                .collect();
            FileCapability {
                name: display_name(&file_path),
                kind: kind_name(file_kind(&file_path)),
                path,
                targets,
            }
        })
        .collect::<Vec<_>>();

    let mut common_targets = files
        .first()
        .map(|file| file.targets.clone())
        .unwrap_or_default();
    common_targets.retain(|target| {
        files
            .iter()
            .skip(1)
            .all(|file| file.targets.contains(target))
    });

    AnalyzeResponse {
        files,
        common_targets,
    }
}

#[tauri::command]
async fn convert_files(paths: Vec<String>, target: String) -> Result<ConversionResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = TargetFormat::from_str(&target)?;
        let inputs = paths.into_iter().map(PathBuf::from).collect::<Vec<_>>();
        let plan = plan_conversion(&inputs, target, None).map_err(|error| error.to_string())?;
        let outputs = ConversionEngine::discover()
            .execute(&plan)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        Ok(ConversionResponse { outputs })
    })
    .await
    .map_err(|error| format!("conversion worker stopped unexpectedly: {error}"))?
}

#[tauri::command]
async fn transfer_files(
    paths: Vec<String>,
    destination: String,
    operation: RequestedTransferOperation,
) -> Result<TransferResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let sources = paths.into_iter().map(PathBuf::from).collect::<Vec<_>>();
        let plan = plan_transfer(
            &sources,
            Path::new(&destination),
            TransferOperation::from(operation),
        )
        .map_err(|error| error.to_string())?;
        let outputs = execute_transfer(&plan)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        Ok(TransferResponse { outputs })
    })
    .await
    .map_err(|error| format!("file operation worker stopped unexpectedly: {error}"))?
}

#[tauri::command]
async fn open_terminal(paths: Vec<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let first = paths.first().ok_or("no path was provided")?;
        let directory = terminal_directory(Path::new(first))?;
        launch_terminal(&directory)
    })
    .await
    .map_err(|error| format!("terminal worker stopped unexpectedly: {error}"))?
}

fn terminal_directory(path: &Path) -> Result<PathBuf, String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("could not resolve {}: {error}", path.display()))?;
    if canonical.is_dir() {
        return Ok(canonical);
    }
    canonical
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("{} has no parent directory", canonical.display()))
}

#[cfg(target_os = "linux")]
fn launch_terminal(directory: &Path) -> Result<(), String> {
    let candidates: &[(&str, &[&str])] = &[
        ("kgx", &["--working-directory"]),
        ("gnome-terminal", &["--working-directory"]),
        ("konsole", &["--workdir"]),
        ("xfce4-terminal", &["--working-directory"]),
        ("x-terminal-emulator", &["--working-directory"]),
    ];

    for (executable, arguments) in candidates {
        match Command::new(executable)
            .args(*arguments)
            .arg(directory)
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("could not start {executable}: {error}")),
        }
    }

    Err("no supported terminal application was found".to_owned())
}

#[cfg(target_os = "windows")]
fn launch_terminal(directory: &Path) -> Result<(), String> {
    match Command::new("wt.exe").arg("-d").arg(directory).spawn() {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Command::new("cmd.exe")
            .current_dir(directory)
            .spawn()
            .map(|_| ())
            .map_err(|error| {
                format!("could not start Windows Terminal or Command Prompt: {error}")
            }),
        Err(error) => Err(format!("could not start Windows Terminal: {error}")),
    }
}

#[cfg(target_os = "macos")]
fn launch_terminal(directory: &Path) -> Result<(), String> {
    Command::new("open")
        .args(["-a", "Terminal"])
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start Terminal: {error}"))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

const fn kind_name(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Image => "image",
        FileKind::Video => "video",
        FileKind::Pdf => "pdf",
        FileKind::TextDocument => "document",
        FileKind::Presentation => "presentation",
        FileKind::Spreadsheet => "spreadsheet",
        FileKind::Unsupported => "unsupported",
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(OverlayRuntime::default())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            system_overlay::setup_linux_desktop_integration(app.handle());
            system_overlay::setup_system_shortcuts(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main"
                && matches!(event, tauri::WindowEvent::CloseRequested { .. })
            {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            analyze_files,
            convert_files,
            transfer_files,
            open_terminal,
            show_overlay,
            hide_overlay,
            current_overlay_mode,
            current_overlay_selection,
            global_shortcut_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running Grapefruit");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_common_targets_for_compatible_selections() {
        let response = analyze_files(vec!["report.docx".into(), "slides.pptx".into()]);
        assert_eq!(response.common_targets, vec!["pdf"]);

        let mixed = analyze_files(vec!["report.docx".into(), "photo.png".into()]);
        assert_eq!(mixed.common_targets, vec!["pdf"]);

        let images = analyze_files(vec!["photo.png".into(), "scan.jpg".into()]);
        assert!(images.common_targets.contains(&"webp".to_owned()));
        assert!(images.common_targets.contains(&"pdf".to_owned()));
    }
}
