#![allow(unsafe_code)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    DVASPECT_CONTENT, FORMATETC, IDataObject, IServiceProvider, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{CF_HDROP, ReleaseStgMedium};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, DragQueryFileW, HDROP, IShellBrowser, IShellWindows, IWebBrowser2,
    SID_STopLevelBrowser, SVGIO_SELECTION, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetForegroundWindow, WindowFromPoint,
};
use windows::core::Interface;

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        // SAFETY: This module owns the dedicated Windows monitor thread's COM apartment.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
            .ok()
            .map_err(|error| format!("could not initialize Windows Shell access: {error}"))?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: Paired with the successful CoInitializeEx call above on the same thread.
        unsafe { CoUninitialize() };
    }
}

/// Returns the files currently selected in the foreground Explorer window or on the desktop.
///
/// Explorer's OLE drag does not reliably attach itself to a transparent WebView window that
/// becomes visible after the drag has started. Reading the source Shell view keeps the quick menu
/// independent from that late drop-target registration.
pub fn selected_paths_at(pointer: (i32, i32)) -> Result<Vec<String>, String> {
    let _apartment = ComApartment::initialize()?;

    // The Windows desktop is not guaranteed to become the foreground window when an icon is
    // dragged. Prefer the window below the pointer, then fall back to the foreground Explorer
    // frame. Both handles are normalized to their top-level roots before class validation.
    let pointer_window = root_window(unsafe {
        WindowFromPoint(POINT {
            x: pointer.0,
            y: pointer.1,
        })
    });
    // SAFETY: GetForegroundWindow has no caller-owned pointers and returns a borrowed HWND value.
    let foreground_window = root_window(unsafe { GetForegroundWindow() });
    let source_window = [pointer_window, foreground_window]
        .into_iter()
        .find(|window| {
            window_class_name(*window).is_some_and(|class_name| {
                is_explorer_class(&class_name) || is_desktop_class(&class_name)
            })
        });
    let Some(source_window) = source_window else {
        return Ok(Vec::new());
    };
    let source_class = window_class_name(source_window).unwrap_or_default();

    // Connect to the Shell only after the source class has been accepted. Since this function is
    // sampled on mouse-down, ordinary clicks and text drags in browsers/editors should return
    // without touching Explorer automation.
    // SAFETY: ShellWindows is a system COM local server. The apartment guard remains alive for
    // every interface used by this function.
    let shell_windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }
        .map_err(|error| format!("could not connect to Windows Explorer: {error}"))?;

    // Limit the COM lookup before touching any cached Shell selection. This keeps ordinary mouse
    // drags in browsers and editors from reusing a selection left behind in Explorer.
    if is_explorer_class(&source_class) {
        // Windows 11 can expose more than one ShellWindows entry for a tabbed Explorer frame. The
        // IWebBrowser2 HWND is the top-level frame, while IShellBrowser::GetWindow may be a tab
        // child. Inspect every entry belonging to the foreground frame and prefer the one with an
        // actual selection instead of returning the first (possibly inactive) tab.
        let count = unsafe { shell_windows.Count() }
            .map_err(|error| format!("could not enumerate Explorer windows: {error}"))?;
        let mut matched_frame = false;
        let mut successful_probe = false;
        let mut last_error = None;

        for index in 0..count {
            let index = VARIANT::from(index);
            // A ShellWindows collection can change while we enumerate it; skip entries that
            // vanished between Count and Item.
            let Ok(dispatch) = (unsafe { shell_windows.Item(&index) }) else {
                continue;
            };
            if dispatch_root_window(&dispatch) != Some(source_window) {
                continue;
            }
            matched_frame = true;

            let browser = match shell_browser(&dispatch) {
                Ok(browser) => browser,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            match paths_from_browser(&browser) {
                Ok(paths) => {
                    successful_probe = true;
                    if !paths.is_empty() {
                        return Ok(paths);
                    }
                }
                Err(error) => last_error = Some(error),
            }
        }

        if successful_probe {
            return Ok(Vec::new());
        }
        if matched_frame {
            return Err(last_error
                .unwrap_or_else(|| "could not read the active Explorer tab selection".to_owned()));
        }
        return Err(format!(
            "could not match Explorer frame class {source_class} to ShellWindows"
        ));
    }

    // The desktop is a Shell view but is not guaranteed to appear in the normal ShellWindows
    // enumeration. Query it explicitly as the fallback source.
    let mut desktop_hwnd = 0;
    let desktop_location = VARIANT::from(CSIDL_DESKTOP as i32);
    let empty_root = VARIANT::default();
    // Microsoft Shell clients identify the desktop with CSIDL_DESKTOP and an empty root variant.
    // Passing null variants can find the desktop window but fail to expose its active Shell view
    // on current Windows 11 builds.
    // SAFETY: FindWindowSW reads the two initialized variants and writes only to the supplied
    // integer handle slot. The returned dispatch remains apartment-bound.
    let desktop = unsafe {
        shell_windows.FindWindowSW(
            &desktop_location,
            &empty_root,
            SWC_DESKTOP,
            &mut desktop_hwnd,
            SWFO_NEEDDISPATCH,
        )
    }
    .map_err(|error| format!("could not access the Windows desktop selection: {error}"))?;
    let browser = shell_browser(&desktop)?;
    paths_from_browser(&browser)
}

fn dispatch_root_window(dispatch: &windows::Win32::System::Com::IDispatch) -> Option<HWND> {
    // IShellWindows::Item exposes an IWebBrowser2 automation object. Its HWND property identifies
    // the top-level Explorer frame, including tabbed Explorer on Windows 11.
    if let Ok(web_browser) = dispatch.cast::<IWebBrowser2>() {
        if let Ok(handle) = unsafe { web_browser.HWND() } {
            let window = HWND(handle.0 as *mut core::ffi::c_void);
            if !window.0.is_null() {
                return Some(root_window(window));
            }
        }
    }

    // Older Shell implementations may not expose IWebBrowser2 directly. Preserve the previous
    // IOleWindow route as a normalized fallback instead of discarding an otherwise usable view.
    let browser = shell_browser(dispatch).ok()?;
    let window = unsafe { browser.GetWindow() }.ok()?;
    (!window.0.is_null()).then(|| root_window(window))
}

fn root_window(window: HWND) -> HWND {
    if window.0.is_null() {
        return window;
    }
    // SAFETY: GetAncestor only inspects the supplied borrowed window handle.
    let root = unsafe { GetAncestor(window, GA_ROOT) };
    if root.0.is_null() { window } else { root }
}

fn window_class_name(window: HWND) -> Option<String> {
    if window.0.is_null() {
        return None;
    }
    let mut buffer = [0_u16; 256];
    // SAFETY: The buffer is writable for its full length and HWND is borrowed from Windows.
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    if length <= 0 {
        return None;
    }
    Some(
        OsString::from_wide(&buffer[..length as usize])
            .to_string_lossy()
            .into_owned(),
    )
}

fn is_explorer_class(class_name: &str) -> bool {
    class_name.eq_ignore_ascii_case("CabinetWClass")
        || class_name.eq_ignore_ascii_case("ExploreWClass")
}

fn is_desktop_class(class_name: &str) -> bool {
    class_name.eq_ignore_ascii_case("Progman") || class_name.eq_ignore_ascii_case("WorkerW")
}

fn shell_browser(
    dispatch: &windows::Win32::System::Com::IDispatch,
) -> Result<IShellBrowser, String> {
    let provider: IServiceProvider = dispatch
        .cast()
        .map_err(|error| format!("Explorer window does not expose Shell services: {error}"))?;
    // SAFETY: QueryService receives the documented top-level browser service identifier and asks
    // windows-rs to materialize the matching IShellBrowser interface.
    unsafe { provider.QueryService(&SID_STopLevelBrowser) }
        .map_err(|error| format!("could not access Explorer's active Shell browser: {error}"))
}

fn paths_from_browser(browser: &IShellBrowser) -> Result<Vec<String>, String> {
    // SAFETY: Both calls operate on interfaces owned by the current COM apartment.
    let view = unsafe { browser.QueryActiveShellView() }
        .map_err(|error| format!("could not access Explorer's active folder view: {error}"))?;
    let data: IDataObject = unsafe { view.GetItemObject(SVGIO_SELECTION) }
        .map_err(|error| format!("could not read the selected Explorer items: {error}"))?;

    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    // SAFETY: FORMATETC requests the standard CF_HDROP HGLOBAL representation. The returned
    // STGMEDIUM is released below regardless of extraction success.
    let mut medium = unsafe { data.GetData(&format) }
        .map_err(|error| format!("selected Explorer items are not file paths: {error}"))?;
    let result = paths_from_medium(&medium);
    // SAFETY: medium was initialized by IDataObject::GetData and has not been released or moved.
    unsafe { ReleaseStgMedium(&mut medium) };
    result
}

fn paths_from_medium(
    medium: &windows::Win32::System::Com::STGMEDIUM,
) -> Result<Vec<String>, String> {
    if medium.tymed != TYMED_HGLOBAL.0 as u32 {
        return Err("Explorer returned an unsupported selection representation".to_owned());
    }

    // SAFETY: The active union field is hGlobal because tymed was checked above.
    let global = unsafe { medium.u.hGlobal };
    if global.0.is_null() {
        return Ok(Vec::new());
    }
    let drop_handle = HDROP(global.0);
    // SAFETY: CF_HDROP guarantees that the HGLOBAL contains a valid DROPFILES payload.
    let count = unsafe { DragQueryFileW(drop_handle, u32::MAX, None) };
    let mut paths = Vec::with_capacity(count as usize);

    for index in 0..count {
        // SAFETY: Passing no destination returns the required UTF-16 length for this item.
        let length = unsafe { DragQueryFileW(drop_handle, index, None) };
        if length == 0 {
            continue;
        }
        let mut buffer = vec![0_u16; length as usize + 1];
        // SAFETY: The buffer is sized from the preceding DragQueryFileW length query.
        let copied = unsafe { DragQueryFileW(drop_handle, index, Some(&mut buffer)) };
        if copied == 0 {
            continue;
        }
        let path = OsString::from_wide(&buffer[..copied as usize])
            .to_string_lossy()
            .into_owned();
        if !path.is_empty() && !paths.contains(&path) {
            paths.push(path);
        }
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_explorer_frame_classes() {
        assert!(is_explorer_class("CabinetWClass"));
        assert!(is_explorer_class("cabinetwclass"));
        assert!(is_explorer_class("ExploreWClass"));
        assert!(!is_explorer_class("Chrome_WidgetWin_1"));
    }

    #[test]
    fn recognizes_only_windows_desktop_root_classes() {
        assert!(is_desktop_class("Progman"));
        assert!(is_desktop_class("WorkerW"));
        assert!(is_desktop_class("workerw"));
        assert!(!is_desktop_class("Notepad"));
    }
}
