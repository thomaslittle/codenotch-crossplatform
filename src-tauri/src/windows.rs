use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Position, Size};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge { Right, Left, Top, Bottom }

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShellStyle {
    Tab,
    Bubble,
    Sharp,
    Trapezoid,
    Pill,
    Rail,
    Dock,
    #[serde(rename = "dock3d")]
    Dock3d,
    Ghost,
}

const SIDE_DEPTH: f64 = 70.0;
const HORIZONTAL_DEPTH: f64 = 84.0;
pub const SLIVER: f64 = 6.0;
const CURL: f64 = 39.0;
const CELL: f64 = 70.0;
const HORIZONTAL_CELL: f64 = 48.0;
const GAP: f64 = 31.0;
const START_PAD: f64 = 26.0;
const END_PAD: f64 = 19.0;
const COMPACT_CELL: f64 = 44.0;
const COMPACT_HORIZONTAL_CELL: f64 = 44.0;
const COMPACT_GAP: f64 = 12.0;
const COMPACT_START_PAD: f64 = 16.0;
const COMPACT_END_PAD: f64 = 14.0;
const TOOLTIP_W: f64 = 270.0;
const TOOLTIP_H: f64 = 280.0;
const AUTODETECT_POLL_MS: u64 = 120;

#[derive(Debug, Clone, Copy)]
struct LayoutMetrics {
    outer: f64,
    start: f64,
    end: f64,
    side_cell: f64,
    horizontal_cell: f64,
    side_gap: f64,
    horizontal_gap: f64,
    side_depth: f64,
    horizontal_depth: f64,
}

fn monitor_rect(monitor: &tauri::Monitor) -> (f64, f64, f64, f64) {
    let scale = monitor.scale_factor();
    let position = monitor.position();
    let size = monitor.size();
    (
        position.x as f64 / scale,
        position.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    )
}

fn primary_rect(app: &AppHandle) -> Result<(f64, f64, f64, f64), String> {
    let monitor = app.primary_monitor().map_err(|e| e.to_string())?.ok_or("No primary monitor")?;
    Ok(monitor_rect(&monitor))
}

fn target_rect(app: &AppHandle, monitor: &str) -> Result<(f64, f64, f64, f64), String> {
    if monitor != "primary" {
        if let Ok(index) = monitor.parse::<usize>() {
            if let Ok(monitors) = app.available_monitors() {
                if let Some(selected) = monitors.get(index) {
                    return Ok(monitor_rect(selected));
                }
            }
        }
    }
    primary_rect(app)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: String,
    pub name: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale_factor: f64,
    pub primary: bool,
}

pub fn list_monitors(app: &AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    let primary_rect = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .as_ref()
        .map(monitor_rect);
    Ok(monitors
        .iter()
        .enumerate()
        .map(|(index, monitor)| {
            let (x, y, width, height) = monitor_rect(monitor);
            MonitorInfo {
                id: index.to_string(),
                name: monitor.name().cloned(),
                x,
                y,
                width,
                height,
                scale_factor: monitor.scale_factor(),
                primary: primary_rect == Some((x, y, width, height)),
            }
        })
        .collect())
}

fn clamp_scale(scale: f64) -> f64 {
    if !scale.is_finite() { return 1.0; }
    scale.clamp(0.5, 2.0)
}

/// Native window geometry has to follow the visible shell, not the original
/// tab's spacing. Dock shells reserve a real Settings slot and intentionally
/// pack provider icons closer together, while legacy/tab-shaped shells keep
/// the original measurements exactly.
fn layout_metrics(compact: bool, shell: ShellStyle) -> LayoutMetrics {
    match shell {
        ShellStyle::Dock | ShellStyle::Rail => {
            if compact {
                LayoutMetrics {
                    outer: 0.0,
                    start: 14.0,
                    end: 54.0,
                    side_cell: 44.0,
                    horizontal_cell: 44.0,
                    side_gap: 8.0,
                    horizontal_gap: 8.0,
                    side_depth: 70.0,
                    horizontal_depth: 80.0,
                }
            } else {
                LayoutMetrics {
                    outer: 0.0,
                    start: 14.0,
                    end: 58.0,
                    side_cell: 70.0,
                    horizontal_cell: 52.0,
                    side_gap: 8.0,
                    horizontal_gap: 10.0,
                    side_depth: 74.0,
                    horizontal_depth: 86.0,
                }
            }
        }
        ShellStyle::Dock3d => {
            if compact {
                LayoutMetrics {
                    outer: 0.0,
                    start: 16.0,
                    end: 58.0,
                    side_cell: 44.0,
                    horizontal_cell: 44.0,
                    side_gap: 7.0,
                    horizontal_gap: 7.0,
                    side_depth: 80.0,
                    horizontal_depth: 90.0,
                }
            } else {
                LayoutMetrics {
                    outer: 0.0,
                    start: 16.0,
                    end: 62.0,
                    side_cell: 70.0,
                    horizontal_cell: 52.0,
                    side_gap: 7.0,
                    horizontal_gap: 7.0,
                    side_depth: 84.0,
                    horizontal_depth: 96.0,
                }
            }
        }
        _ => {
            if compact {
                LayoutMetrics {
                    outer: CURL,
                    start: COMPACT_START_PAD,
                    end: COMPACT_END_PAD,
                    side_cell: COMPACT_CELL,
                    horizontal_cell: COMPACT_HORIZONTAL_CELL,
                    side_gap: COMPACT_GAP,
                    horizontal_gap: COMPACT_GAP,
                    side_depth: SIDE_DEPTH,
                    horizontal_depth: HORIZONTAL_DEPTH,
                }
            } else {
                LayoutMetrics {
                    outer: CURL,
                    start: START_PAD,
                    end: END_PAD,
                    side_cell: CELL,
                    horizontal_cell: HORIZONTAL_CELL,
                    side_gap: GAP,
                    horizontal_gap: GAP,
                    side_depth: SIDE_DEPTH,
                    horizontal_depth: HORIZONTAL_DEPTH,
                }
            }
        }
    }
}

fn side_length(count: usize, scale: f64, compact: bool, shell: ShellStyle) -> f64 {
    let count = count.max(1) as f64;
    let m = layout_metrics(compact, shell);
    (m.outer * 2.0 + m.start + m.end + count * m.side_cell + (count - 1.0) * m.side_gap) * scale
}

fn horizontal_length(count: usize, scale: f64, compact: bool, shell: ShellStyle) -> f64 {
    let count = count.max(1) as f64;
    let m = layout_metrics(compact, shell);
    (m.outer * 2.0 + m.start + m.end + count * m.horizontal_cell + (count - 1.0) * m.horizontal_gap) * scale
}

fn provider_center(index: usize, edge: Edge, scale: f64, compact: bool, shell: ShellStyle) -> f64 {
    let m = layout_metrics(compact, shell);
    let (cell, gap) = match edge {
        Edge::Right | Edge::Left => (m.side_cell, m.side_gap),
        Edge::Top | Edge::Bottom => (m.horizontal_cell, m.horizontal_gap),
    };
    (m.outer + m.start + index as f64 * (cell + gap) + cell / 2.0) * scale
}

pub fn place_notch(
    app: &AppHandle,
    edge: Edge,
    count: usize,
    scale: f64,
    monitor: &str,
    offset_x: f64,
    offset_y: f64,
    compact: bool,
    shell: ShellStyle,
) -> Result<(), String> {
    let window = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let s = clamp_scale(scale);
    let (mx, my, mw, mh) = target_rect(app, monitor)?;
    let metrics = layout_metrics(compact, shell);
    let side_depth = metrics.side_depth * s;
    let horizontal_depth = metrics.horizontal_depth * s;
    let (w, h, mut x, mut y) = match edge {
        Edge::Right => {
            let h = side_length(count, s, compact, shell).min(mh - 32.0);
            (side_depth, h, mx + mw - side_depth, my + (mh - h) / 2.0)
        }
        Edge::Left => {
            let h = side_length(count, s, compact, shell).min(mh - 32.0);
            (side_depth, h, mx, my + (mh - h) / 2.0)
        }
        Edge::Top => {
            let w = horizontal_length(count, s, compact, shell).min(mw - 32.0);
            (w, horizontal_depth, mx + (mw - w) / 2.0, my)
        }
        Edge::Bottom => {
            let w = horizontal_length(count, s, compact, shell).min(mw - 32.0);
            (w, horizontal_depth, mx + (mw - w) / 2.0, my + mh - horizontal_depth)
        }
    };
    let margin_x = 80.0_f64.min(w);
    let margin_y = 80.0_f64.min(h);
    x = (x + offset_x.clamp(-2000.0, 2000.0)).clamp(mx - w + margin_x, mx + mw - margin_x);
    y = (y + offset_y.clamp(-2000.0, 2000.0)).clamp(my - h + margin_y, my + mh - margin_y);
    window.set_size(Size::Logical(LogicalSize::new(w, h))).map_err(|e| e.to_string())?;
    window.set_position(Position::Logical(LogicalPosition::new(x, y))).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn place_tooltip(
    app: &AppHandle,
    edge: Edge,
    index: usize,
    notch_scale: f64,
    compact: bool,
    shell: ShellStyle,
) -> Result<(), String> {
    let notch = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let tooltip = app.get_webview_window("tooltip").ok_or("Tooltip window missing")?;
    let factor = notch.scale_factor().map_err(|e| e.to_string())?;
    let p = notch.outer_position().map_err(|e| e.to_string())?;
    let s = notch.outer_size().map_err(|e| e.to_string())?;
    let nx = p.x as f64 / factor;
    let ny = p.y as f64 / factor;
    let nw = s.width as f64 / factor;
    let nh = s.height as f64 / factor;
    tooltip.set_size(Size::Logical(LogicalSize::new(TOOLTIP_W, TOOLTIP_H))).map_err(|e| e.to_string())?;
    let k = clamp_scale(notch_scale);
    let (mx, my, mw, mh) = notch
        .current_monitor()
        .ok()
        .flatten()
        .as_ref()
        .map(monitor_rect)
        .or_else(|| primary_rect(app).ok())
        .unwrap_or((0.0, 0.0, 2560.0, 1440.0));

    let (x, y) = match edge {
        Edge::Right | Edge::Left => {
            let center = provider_center(index, edge, k, compact, shell);
            let y = (ny + center - TOOLTIP_H / 2.0).clamp(my, (my + mh - TOOLTIP_H).max(my));
            let x = match edge { Edge::Right => nx - TOOLTIP_W + 9.0, _ => nx + nw - 9.0 };
            (x, y)
        }
        Edge::Top | Edge::Bottom => {
            let center = provider_center(index, edge, k, compact, shell);
            let x = (nx + center - TOOLTIP_W / 2.0).clamp(mx, (mx + mw - TOOLTIP_W).max(mx));
            let y = match edge { Edge::Top => ny + nh - 9.0, _ => ny - TOOLTIP_H + 9.0 };
            (x, y)
        }
    };
    tooltip.set_position(Position::Logical(LogicalPosition::new(x, y))).map_err(|e| e.to_string())?;
    tooltip.show().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn open_settings(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("settings").ok_or("Settings window missing")?;
    window.center().map_err(|e| e.to_string())?;
    window.set_size(Size::Logical(LogicalSize::new(620.0, 660.0))).map_err(|e| e.to_string())?;
    window.center().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn place_context_menu(app: &AppHandle, edge: Edge, notch_scale: f64) -> Result<(), String> {
    let notch = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let menu = app.get_webview_window("context-menu").ok_or("Context menu window missing")?;
    let factor = notch.scale_factor().map_err(|e| e.to_string())?;
    let p = notch.outer_position().map_err(|e| e.to_string())?;
    let s = notch.outer_size().map_err(|e| e.to_string())?;
    let nx = p.x as f64 / factor;
    let ny = p.y as f64 / factor;
    let nw = s.width as f64 / factor;
    let nh = s.height as f64 / factor;
    let k = clamp_scale(notch_scale);
    let (x, y) = match edge {
        Edge::Right => (nx - 194.0 * k, ny + nh / 2.0 - 82.0 * k),
        Edge::Left => (nx + nw - 6.0 * k, ny + nh / 2.0 - 82.0 * k),
        Edge::Top => (nx + nw / 2.0 - 90.0 * k, ny + nh - 6.0 * k),
        Edge::Bottom => (nx + nw / 2.0 - 90.0 * k, ny - 158.0 * k),
    };
    menu.set_position(Position::Logical(LogicalPosition::new(x, y))).map_err(|e| e.to_string())?;
    menu.show().map_err(|e| e.to_string())?;
    menu.set_focus().ok();
    Ok(())
}

pub fn cursor_inside_notch_or_tooltip(app: &AppHandle) -> Option<bool> {
    #[cfg(target_os = "windows")]
    {
        let (cx, cy) = cursor_position_global()?;
        for label in ["notch", "tooltip"] {
            let window = app.get_webview_window(label)?;
            let position = window.outer_position().ok()?;
            let size = window.outer_size().ok()?;
            if !window.is_visible().unwrap_or(false) {
                continue;
            }
            let (x, y) = (position.x, position.y);
            let (w, h) = (size.width as i32, size.height as i32);
            if cx >= x - 2 && cx < x + w + 2 && cy >= y - 2 && cy < y + h + 2 {
                return Some(true);
            }
        }
        Some(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        None
    }
}

pub fn cursor_inside_overlay(app: &AppHandle) -> Option<bool> {
    let (cx, cy) = cursor_position_global()?;
    for label in ["tooltip", "context-menu", "settings"] {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        if !window.is_visible().unwrap_or(false) {
            continue;
        }
        let position = window.outer_position().ok()?;
        let size = window.outer_size().ok()?;
        let (x, y) = (position.x, position.y);
        let (w, h) = (size.width as i32, size.height as i32);
        if cx >= x - 2 && cx < x + w + 2 && cy >= y - 2 && cy < y + h + 2 {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(target_os = "windows")]
pub fn cursor_position_global() -> Option<(i32, i32)> {
    #[repr(C)]
    struct Point { x: i32, y: i32 }
    #[link(name = "user32")]
    extern "system" { fn GetCursorPos(point: *mut Point) -> i32; }
    let mut point = Point { x: 0, y: 0 };
    let ok = unsafe { GetCursorPos(&mut point) };
    (ok != 0).then_some((point.x, point.y))
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
    use std::sync::OnceLock;

    #[link(name = "X11")]
    extern "C" {
        fn XOpenDisplay(display_name: *const c_char) -> *mut c_void;
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XQueryPointer(
            display: *mut c_void,
            window: c_ulong,
            root_return: *mut c_ulong,
            child_return: *mut c_ulong,
            root_x_return: *mut c_int,
            root_y_return: *mut c_int,
            win_x_return: *mut c_int,
            win_y_return: *mut c_int,
            mask_return: *mut c_uint,
        ) -> c_int;
    }

    static DISPLAY: OnceLock<Option<usize>> = OnceLock::new();

    pub fn display() -> Option<*mut c_void> {
        let cached = DISPLAY.get_or_init(|| {
            let display = unsafe { XOpenDisplay(std::ptr::null()) };
            (!display.is_null()).then_some(display as usize)
        });
        (*cached).map(|address| address as *mut c_void)
    }

    pub fn cursor_position() -> Option<(i32, i32)> {
        let display = display()?;
        let root = unsafe { XDefaultRootWindow(display) };
        let mut root_return = 0;
        let mut child_return = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;
        let ok = unsafe {
            XQueryPointer(
                display,
                root,
                &mut root_return,
                &mut child_return,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            )
        };
        (ok != 0).then_some((root_x, root_y))
    }
}

#[cfg(target_os = "linux")]
pub fn cursor_position_global() -> Option<(i32, i32)> { x11::cursor_position() }

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn cursor_position_global() -> Option<(i32, i32)> { None }

pub fn autohide_supported() -> bool {
    #[cfg(target_os = "windows")]
    { return true; }
    #[cfg(target_os = "linux")]
    { return x11::display().is_some(); }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    { false }
}

pub fn hotspot_rect(x: i32, y: i32, w: i32, h: i32, edge: Edge, depth_phys: i32) -> (i32, i32, i32, i32) {
    let w = w.max(0);
    let h = h.max(0);
    match edge {
        Edge::Right => { let depth = depth_phys.clamp(0, w); (x + w - depth, y, depth, h) }
        Edge::Left => { let depth = depth_phys.clamp(0, w); (x, y, depth, h) }
        Edge::Top => { let depth = depth_phys.clamp(0, h); (x, y, w, depth) }
        Edge::Bottom => { let depth = depth_phys.clamp(0, h); (x, y + h - depth, w, depth) }
    }
}

fn point_in_rect(px: i32, py: i32, rect: (i32, i32, i32, i32)) -> bool {
    let (x, y, w, h) = rect;
    px >= x && px < x + w && py >= y && py < y + h
}

pub fn set_notch_retracted(app: &AppHandle, retracted: bool, edge: Edge) -> Result<(), String> {
    let state = app.state::<crate::AutohideState>();
    let previous = state.retracted.swap(retracted, Ordering::AcqRel);
    if previous == retracted { return Ok(()); }

    let window = app.get_webview_window("notch").ok_or_else(|| {
        state.retracted.store(previous, Ordering::Release);
        "Notch window missing".to_string()
    })?;
    if let Err(error) = window.set_ignore_cursor_events(retracted) {
        state.retracted.store(previous, Ordering::Release);
        return Err(error.to_string());
    }

    if retracted { spawn_retract_poll(app.clone(), edge); }
    Ok(())
}

fn spawn_retract_poll(app: AppHandle, edge: Edge) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(AUTODETECT_POLL_MS)).await;
            if !app.state::<crate::AutohideState>().retracted.load(Ordering::Acquire) { break; }
            let Some(window) = app.get_webview_window("notch") else { break; };
            if !window.is_visible().unwrap_or(false) { break; }
            let Some((cx, cy)) = cursor_position_global() else { continue; };
            let Ok(position) = window.outer_position() else { continue; };
            let Ok(size) = window.outer_size() else { continue; };
            let Ok(scale_factor) = window.scale_factor() else { continue; };
            let width = i32::try_from(size.width).unwrap_or(i32::MAX);
            let height = i32::try_from(size.height).unwrap_or(i32::MAX);
            let depth = (SLIVER * 2.0 * scale_factor).round().max(1.0) as i32;
            let hotspot = hotspot_rect(position.x, position.y, width, height, edge, depth);
            if !point_in_rect(cx, cy, hotspot) { continue; }
            if set_notch_retracted(&app, false, edge).is_ok() {
                let _ = app.emit_to("notch", "notch:peek", ());
                break;
            }
        }
    });
}

pub fn fit_settings(app: &AppHandle, height: f64) -> Result<(), String> {
    let window = app.get_webview_window("settings").ok_or("Settings window missing")?;
    let mh = window
        .current_monitor()
        .ok()
        .flatten()
        .as_ref()
        .map(monitor_rect)
        .or_else(|| primary_rect(app).ok())
        .map(|(_, _, _, mh)| mh)
        .unwrap_or(900.0);
    let h = height.clamp(420.0, (mh - 60.0).max(420.0));
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let w = size.width as f64 / scale;
    window.set_size(Size::Logical(LogicalSize::new(w, h))).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{horizontal_length, hotspot_rect, layout_metrics, provider_center, side_length, Edge, ShellStyle};

    #[test]
    fn hotspot_hugs_right_edge() {
        assert_eq!(hotspot_rect(10, 20, 100, 50, Edge::Right, 12), (98, 20, 12, 50));
    }

    #[test]
    fn hotspot_hugs_left_edge() {
        assert_eq!(hotspot_rect(10, 20, 100, 50, Edge::Left, 12), (10, 20, 12, 50));
    }

    #[test]
    fn hotspot_hugs_top_edge() {
        assert_eq!(hotspot_rect(10, 20, 100, 50, Edge::Top, 12), (10, 20, 100, 12));
    }

    #[test]
    fn hotspot_hugs_bottom_edge() {
        assert_eq!(hotspot_rect(10, 20, 100, 50, Edge::Bottom, 12), (10, 58, 100, 12));
    }

    #[test]
    fn glass_dock_packs_four_classic_icons() {
        assert_eq!(horizontal_length(4, 1.0, false, ShellStyle::Dock), 310.0);
        assert_eq!(provider_center(0, Edge::Bottom, 1.0, false, ShellStyle::Dock), 40.0);
        assert_eq!(provider_center(3, Edge::Bottom, 1.0, false, ShellStyle::Dock), 226.0);
    }

    #[test]
    fn three_d_dock_has_its_own_depth_and_spacing() {
        let metrics = layout_metrics(false, ShellStyle::Dock3d);
        assert_eq!(metrics.horizontal_depth, 96.0);
        assert_eq!(metrics.side_depth, 84.0);
        assert_eq!(horizontal_length(4, 1.0, false, ShellStyle::Dock3d), 307.0);
        assert_eq!(side_length(4, 1.0, false, ShellStyle::Dock3d), 379.0);
    }

    #[test]
    fn tab_geometry_is_unchanged() {
        assert_eq!(horizontal_length(4, 1.0, false, ShellStyle::Tab), 408.0);
    }
}
