#![allow(unsafe_code)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    DVASPECT_CONTENT, FORMATETC, IDataObject, IServiceProvider, TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{CF_HDROP, ReleaseStgMedium};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    DragQueryFileW, HDROP, IShellBrowser, IShellWindows, SID_STopLevelBrowser, SVGIO_SELECTION,
    SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};
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
pub fn selected_paths() -> Result<Vec<String>, String> {
    let _apartment = ComApartment::initialize()?;

    // SAFETY: ShellWindows is a system COM local server. The apartment guard remains alive for
    // every interface used by this function.
    let shell_windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }
        .map_err(|error| format!("could not connect to Windows Explorer: {error}"))?;

    // SAFETY: GetForegroundWindow has no caller-owned pointers and returns a borrowed HWND value.
    let foreground = unsafe { GetForegroundWindow() };
    // SAFETY: Count and Item are called on a valid IShellWindows interface in this apartment.
    let count = unsafe { shell_windows.Count() }
        .map_err(|error| format!("could not enumerate Explorer windows: {error}"))?;

    for index in 0..count {
        let index = VARIANT::from(index);
        // A ShellWindows collection can change while we enumerate it; skip entries that vanished.
        let Ok(dispatch) = (unsafe { shell_windows.Item(&index) }) else {
            continue;
        };
        let Ok(browser) = shell_browser(&dispatch) else {
            continue;
        };
        // SAFETY: IShellBrowser inherits IOleWindow; GetWindow only returns its owned HWND.
        let Ok(window) = (unsafe { browser.GetWindow() }) else {
            continue;
        };
        if window == foreground {
            return paths_from_browser(&browser);
        }
    }

    // Never fall back to the desktop merely because it still has a selected icon. Without this
    // foreground check, selecting text in an unrelated application could reuse that stale desktop
    // selection and incorrectly open Grapefruit.
    if !is_desktop_window(foreground) {
        return Ok(Vec::new());
    }

    // The desktop is a Shell view but is not guaranteed to appear in the normal ShellWindows
    // enumeration. Query it explicitly as the fallback source.
    let mut desktop_hwnd = 0;
    // SAFETY: FindWindowSW accepts null location variants for SWC_DESKTOP and writes only to the
    // supplied integer handle slot. The returned dispatch remains apartment-bound.
    let desktop = unsafe {
        shell_windows.FindWindowSW(
            std::ptr::null(),
            std::ptr::null(),
            SWC_DESKTOP,
            &mut desktop_hwnd,
            SWFO_NEEDDISPATCH,
        )
    }
    .map_err(|error| format!("could not access the Windows desktop selection: {error}"))?;
    let browser = shell_browser(&desktop)?;
    paths_from_browser(&browser)
}

fn is_desktop_window(window: windows::Win32::Foundation::HWND) -> bool {
    if window.0.is_null() {
        return false;
    }
    let mut buffer = [0_u16; 256];
    // SAFETY: The buffer is writable for its full length and HWND is borrowed from Windows.
    let length = unsafe { GetClassNameW(window, &mut buffer) };
    if length <= 0 {
        return false;
    }
    let class_name = OsString::from_wide(&buffer[..length as usize])
        .to_string_lossy()
        .into_owned();
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
