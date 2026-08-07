//! Lightweight glance window geometry: a single Tauri command that
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

// Only the Windows-only dock/restore commands below touch LogicalSize; gate it
// so non-Windows (CI) doesn't see an unused import.
#[cfg(target_os = "windows")]
use tauri::LogicalSize;

/// Full-mode minimum CLIENT size (logical px). The dashboard never shrinks below
/// this, keeping it clearly larger than the glance card's fixed small shapes.
/// Declared at window creation (`tauri.conf.json` minWidth/minHeight) and
/// re-applied by the full-mode restore commands; the lightweight dock clears it
/// (min 0 ⇒ no constraint) so the glance card can reach its fixed small size.
///
/// Must match `window-shapes.ts` `MIN_FULL` (front-end restore clamp) and
/// `tauri.conf.json` minWidth/minHeight (creation-time OS floor) — the restore
/// commands re-apply this value with `set_min_size`, overriding all three.
#[cfg(target_os = "windows")]
const FULL_MIN_W: f64 = 840.0;
#[cfg(target_os = "windows")]
const FULL_MIN_H: f64 = 600.0;

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
        let _ = (
            window,
            client_logical_w,
            client_logical_h,
            logical_y,
            inset_logical,
        );
        Err("dock_window_right is only supported on Windows".into())
    }
}

/// Restore the window if maximized, then read its live shadow insets
/// (outer − client; client.left/top == 0). Shared by the dock + center
/// commands: both need correct insets and a non-maximized window before they
/// measure/position via SetWindowPos.
#[cfg(target_os = "windows")]
fn win_shadow_insets(hwnd: windows::Win32::Foundation::HWND) -> Result<(i32, i32), String> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsZoomed, ShowWindow, SW_RESTORE,
    };
    // A maximized window's GetWindowRect overflows its monitor — Windows pads a
    // hidden border margin on every side — which inflates the shadow insets;
    // restore first.
    if unsafe { IsZoomed(hwnd) }.as_bool() {
        // Best-effort; failure to restore is non-fatal (the measure would just
        // be slightly off, not crash).
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
    }
    let mut wrect = RECT::default();
    let mut crect = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut wrect).map_err(|e| e.to_string())?;
        GetClientRect(hwnd, &mut crect).map_err(|e| e.to_string())?;
    }
    Ok((
        (wrect.right - wrect.left) - crect.right,
        (wrect.bottom - wrect.top) - crect.bottom,
    ))
}

/// Measured window geometry shared by the dock / center / set-rect commands:
/// the live scale factor, the target OUTER size (client + shadow) in physical
/// px, and the monitor Windows considers the window to be on. Each command
/// computes only its own outer top-left from this, then hands the rect to
/// [`set_outer_rect`] for one atomic `SetWindowPos`.
#[cfg(target_os = "windows")]
struct WindowPlacement {
    hwnd: windows::Win32::Foundation::HWND,
    scale: f64,
    target_outer_w: i32,
    target_outer_h: i32,
    mon: windows::Win32::Foundation::RECT,
}

/// Read hwnd / scale / live shadow insets / monitor for a desired CLIENT size.
/// Restore-if-maximized happens inside [`win_shadow_insets`] so the measure is
/// correct before any positioning.
#[cfg(target_os = "windows")]
fn measure_window(
    window: &WebviewWindow,
    client_logical_w: f64,
    client_logical_h: f64,
) -> Result<WindowPlacement, String> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };

    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let (shadow_lr, shadow_tb) = win_shadow_insets(hwnd)?;

    let target_client_w = (client_logical_w * scale).round() as i32;
    let target_client_h = (client_logical_h * scale).round() as i32;

    // Pick the monitor the Windows way: largest intersection area.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut mi) }.as_bool() {
        return Err("GetMonitorInfoW failed".into());
    }

    Ok(WindowPlacement {
        hwnd,
        scale,
        target_outer_w: target_client_w + shadow_lr,
        target_outer_h: target_client_h + shadow_tb,
        mon: mi.rcMonitor,
    })
}

/// Apply an OUTER rect in one atomic `SetWindowPos` (size + position together).
/// hwndInsertAfter is ignored under SWP_NOZORDER, so None.
#[cfg(target_os = "windows")]
fn set_outer_rect(p: &WindowPlacement, outer_x: i32, outer_y: i32) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    unsafe {
        SetWindowPos(
            p.hwnd,
            None,
            outer_x,
            outer_y,
            p.target_outer_w,
            p.target_outer_h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn dock_right_win(
    window: &WebviewWindow,
    client_logical_w: f64,
    client_logical_h: f64,
    logical_y: f64,
    inset_logical: f64,
) -> Result<f64, String> {
    // The glance card docks well below the full-mode minimum — drop the min
    // (0 ⇒ no constraint) so the SetWindowPos below isn't clamped back up. The
    // full-mode restore commands re-apply it.
    let _ = window.set_min_size(Some(LogicalSize::new(0.0, 0.0)));
    let p = measure_window(window, client_logical_w, client_logical_h)?;
    let inset_phys = (inset_logical * p.scale).ceil() as i32;
    let outer_x = p.mon.right - inset_phys - p.target_outer_w;
    let lo = p.mon.top + inset_phys;
    let hi = p.mon.bottom - inset_phys - p.target_outer_h;
    let desired_y = p.mon.top + (logical_y * p.scale).round() as i32;
    let outer_y = desired_y.clamp(lo.min(hi), lo.max(hi));
    set_outer_rect(&p, outer_x, outer_y)?;
    Ok(((outer_y - p.mon.top) as f64) / p.scale)
}

/// Center the window on its current monitor at a given CLIENT size, in one
/// atomic `SetWindowPos` (size + position together). Used by the lightweight →
/// full restore. Like `dock_window_right`, the single `SetWindowPos` avoids the
/// `[new size, old pos]` straddle that would flip `MonitorFromWindow` to a
/// neighbour of different DPI and lock WebView2 to the wrong rasterization
/// scale (content renders too small on high-DPI multi-monitor setups).
#[tauri::command]
#[specta::specta]
pub fn center_window(
    window: WebviewWindow,
    client_logical_w: f64,
    client_logical_h: f64,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        center_window_win(&window, client_logical_w, client_logical_h)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, client_logical_w, client_logical_h);
        Err("center_window is only supported on Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn center_window_win(
    window: &WebviewWindow,
    client_logical_w: f64,
    client_logical_h: f64,
) -> Result<(), String> {
    // Re-apply the full-mode minimum (the lightweight dock cleared it) BEFORE
    // SetWindowPos so the restore is clamped up to it, never undersized.
    let _ = window.set_min_size(Some(LogicalSize::new(FULL_MIN_W, FULL_MIN_H)));
    let p = measure_window(window, client_logical_w, client_logical_h)?;
    let outer_x = p.mon.left + (p.mon.right - p.mon.left - p.target_outer_w) / 2;
    let outer_y = p.mon.top + (p.mon.bottom - p.mon.top - p.target_outer_h) / 2;
    set_outer_rect(&p, outer_x, outer_y)
}

/// Place the window at an arbitrary logical rect, in one atomic `SetWindowPos`
/// (size + position together). Restores the full-mode window to the position
/// and size the user last left it at. Like the dock and center commands, the
/// single `SetWindowPos` avoids the `[new size, old pos]` straddle that flips
/// `MonitorFromWindow` to a neighbour of different DPI and locks WebView2 to
/// the wrong rasterization scale.
///
/// `logical_x/y` is the desired CLIENT top-left in logical px relative to the
/// virtual-screen origin (what `outerPosition() / scaleFactor` produces);
/// `logical_w/h` is the desired CLIENT size. The full OUTER rect (shadow
/// included) is clamped to the monitor Windows considers the window to be on,
/// so a stored position that no longer fits — monitor removed, or the window
/// travelled to another monitor while lightweight — lands on-screen. No return
/// value: the caller tracks position. Windows-only; errors elsewhere.
#[tauri::command]
#[specta::specta]
pub fn set_window_rect(
    window: WebviewWindow,
    logical_x: f64,
    logical_y: f64,
    logical_w: f64,
    logical_h: f64,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_window_rect_win(&window, logical_x, logical_y, logical_w, logical_h)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, logical_x, logical_y, logical_w, logical_h);
        Err("set_window_rect is only supported on Windows".into())
    }
}

#[cfg(target_os = "windows")]
fn set_window_rect_win(
    window: &WebviewWindow,
    logical_x: f64,
    logical_y: f64,
    logical_w: f64,
    logical_h: f64,
) -> Result<(), String> {
    // Re-apply the full-mode minimum (the lightweight dock cleared it) BEFORE
    // SetWindowPos so the restore is clamped up to it, never undersized.
    let _ = window.set_min_size(Some(LogicalSize::new(FULL_MIN_W, FULL_MIN_H)));
    let p = measure_window(window, logical_w, logical_h)?;
    // Desired outer top-left in physical virtual-screen coords, clamped so the
    // full outer rect (shadow included) stays inside the current monitor.
    let raw_x = (logical_x * p.scale).round() as i32;
    let raw_y = (logical_y * p.scale).round() as i32;
    let lo_x = p.mon.left;
    let hi_x = p.mon.right - p.target_outer_w;
    let lo_y = p.mon.top;
    let hi_y = p.mon.bottom - p.target_outer_h;
    let outer_x = raw_x.clamp(lo_x.min(hi_x), lo_x.max(hi_x));
    let outer_y = raw_y.clamp(lo_y.min(hi_y), lo_y.max(hi_y));
    set_outer_rect(&p, outer_x, outer_y)
}

#[cfg(test)]
mod tests {
    // FULL_MIN_W / FULL_MIN_H exist only on Windows (compiled out elsewhere);
    // import them conditionally so non-Windows CI has no unused import.
    #[cfg(target_os = "windows")]
    use super::{FULL_MIN_H, FULL_MIN_W};

    /// The full-mode restore commands re-apply `set_min_size(FULL_MIN_W, FULL_MIN_H)`
    /// (clearing it first in the lightweight dock), which overrides the OS floor
    /// declared at window creation — so this constant must agree with the
    /// `minWidth`/`minHeight` in `tauri.conf.json`, or the dashboard can be
    /// restored smaller than the declared minimum. (It drifted to 720×520 once
    /// while the conf already said 840×600.) Mirrors `window-shapes.test.ts`,
    /// which pins the front-end `MIN_FULL` to the same declaration.
    #[test]
    fn full_min_matches_tauri_conf_declaration() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json parses");
        let windows = conf["app"]["windows"]
            .as_array()
            .expect("app.windows array");
        let main = windows
            .iter()
            .find(|w| w["label"] == "main")
            .expect("main window entry");
        #[cfg(target_os = "windows")]
        {
            let min_w = main["minWidth"].as_f64().expect("minWidth numeric");
            let min_h = main["minHeight"].as_f64().expect("minHeight numeric");
            assert_eq!(FULL_MIN_W, min_w);
            assert_eq!(FULL_MIN_H, min_h);
        }
        // On non-Windows builds the constant is compiled out; the conf declaration
        // is the only floor, and the assertion above can't run.
    }
}
