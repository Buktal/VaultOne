//! Lightweight glance window geometry (ADR-0015): a single Tauri command that
//! docks the main window flush against the right edge of the monitor Windows
//! considers it to be on, sizing its OUTER rect in one atomic `SetWindowPos`.
//!
//! Why this exists (and why the previous JS-side dock didn't work):
//! - With `decorations:false, shadow:true`, tao treats `setSize(PhysicalSize)`
//!   as the CLIENT size and adds the shadow margin on top, so the outer rect
//!   overshoots the monitor edge by the shadow width (~15 phys px @ 150% — the
//!   "about 1/5 on the other screen" symptom).
//! - `setPosition` + `setSize` are two async `SetWindowPos` calls; between them
//!   the window briefly sits at [new pos, old size], which for the tuck
//!   direction overshoots the edge by hundreds of px and flips
//!   `MonitorFromWindow` to the neighbour monitor → `WM_DPICHANGED` → WebView2
//!   locks its rasterization scale to the wrong DPI.
//!
//! This command reads the live shadow insets (outer − client), computes the
//! outer rect that keeps the FULL outer rect (shadow included) inside one
//! monitor, and applies it in a single `SetWindowPos` — no intermediate
//! straddling state. The monitor is picked with `MonitorFromWindow`, matching
//! Windows' "largest intersection area" rule (the old JS used the window
//! center, which disagreed with Windows at an A/B edge).

use tauri::WebviewWindow;

/// Dock the given window against the right edge of its current monitor.
///
/// `client_logical_w/h` is the desired CLIENT (visible content) size in logical
/// px; `logical_y` is the desired client top in logical px relative to the
/// monitor top; `inset_logical` is how far the OUTER rect is kept inside the
/// monitor edge. Returns the clamped logical y so callers can remember it.
///
/// Windows-only; on other targets it returns an error (the app only ships on
/// Windows, but the crate still has to compile elsewhere for dev/CI).
#[tauri::command]
#[specta::specta]
pub fn dock_window_right(
    window: WebviewWindow,
    client_logical_w: f64,
    client_logical_h: f64,
    logical_y: f64,
    inset_logical: f64,
) -> Result<f64, String> {
    #[cfg(target_os = "windows")]
    {
        dock_right_win(
            &window,
            client_logical_w,
            client_logical_h,
            logical_y,
            inset_logical,
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, client_logical_w, client_logical_h, logical_y, inset_logical);
        Err("dock_window_right is only supported on Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn dock_right_win(
    window: &WebviewWindow,
    client_logical_w: f64,
    client_logical_h: f64,
    logical_y: f64,
    inset_logical: f64,
) -> Result<f64, String> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;

    // Live shadow insets = outer rect − client rect (client.left/top == 0).
    let mut wrect = RECT::default();
    let mut crect = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut wrect).map_err(|e| e.to_string())?;
        GetClientRect(hwnd, &mut crect).map_err(|e| e.to_string())?;
    }
    let shadow_lr = (wrect.right - wrect.left) - crect.right;
    let shadow_tb = (wrect.bottom - wrect.top) - crect.bottom;

    let target_client_w = (client_logical_w * scale).round() as i32;
    let target_client_h = (client_logical_h * scale).round() as i32;
    let target_outer_w = target_client_w + shadow_lr;
    let target_outer_h = target_client_h + shadow_tb;

    // Pick the monitor the Windows way: largest intersection area.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
        return Err("GetMonitorInfoW failed".into());
    }
    let mon = mi.rcMonitor;

    let inset_phys = (inset_logical * scale).ceil() as i32;
    let outer_x = mon.right - inset_phys - target_outer_w;

    let lo = mon.top + inset_phys;
    let hi = mon.bottom - inset_phys - target_outer_h;
    let desired_y = mon.top + (logical_y * scale).round() as i32;
    let outer_y = desired_y.clamp(lo.min(hi), lo.max(hi));

    // hwndInsertAfter is ignored under SWP_NOZORDER; pass None.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            outer_x,
            outer_y,
            target_outer_w,
            target_outer_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(((outer_y - mon.top) as f64) / scale)
}
