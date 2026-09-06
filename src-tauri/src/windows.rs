use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Position, Size};

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge { Right, Left, Top, Bottom }

const SIDE_DEPTH: f64 = 70.0;
const HORIZONTAL_DEPTH: f64 = 70.0;
const CURL: f64 = 39.0;
const RING: f64 = 44.0;
const CELL: f64 = 70.0;
const HORIZONTAL_CELL: f64 = 48.0;
const GAP: f64 = 31.0;
const START_PAD: f64 = 26.0;
const END_PAD: f64 = 19.0;
const TOOLTIP_W: f64 = 270.0;
const TOOLTIP_H: f64 = 280.0;

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

/// Resolve `"primary"` or a monitor index (see `list_monitors`) to logical
/// geometry, falling back to the primary monitor.
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

fn side_length(count: usize, scale: f64) -> f64 {
    let count = count.max(1) as f64;
    let (curl, start, end, cell, gap) = (CURL * scale, START_PAD * scale, END_PAD * scale, CELL * scale, GAP * scale);
    curl * 2.0 + start + end + count * cell + (count - 1.0) * gap
}

fn horizontal_length(count: usize, scale: f64) -> f64 {
    let count = count.max(1) as f64;
    let (curl, start, end, cell, gap) = (CURL * scale, START_PAD * scale, END_PAD * scale, HORIZONTAL_CELL * scale, GAP * scale);
    curl * 2.0 + start + end + count * cell + (count - 1.0) * gap
}

pub fn place_notch(
    app: &AppHandle,
    edge: Edge,
    count: usize,
    scale: f64,
    monitor: &str,
    offset_x: f64,
    offset_y: f64,
) -> Result<(), String> {
    let window = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let s = clamp_scale(scale);
    let (mx, my, mw, mh) = target_rect(app, monitor)?;
    let side_depth = SIDE_DEPTH * s;
    let horizontal_depth = HORIZONTAL_DEPTH * s;
    let (w, h, mut x, mut y) = match edge {
        Edge::Right => {
            let h = side_length(count, s).min(mh - 32.0);
            (side_depth, h, mx + mw - side_depth, my + (mh - h) / 2.0)
        }
        Edge::Left => {
            let h = side_length(count, s).min(mh - 32.0);
            (side_depth, h, mx, my + (mh - h) / 2.0)
        }
        Edge::Top => {
            let w = horizontal_length(count, s).min(mw - 32.0);
            (w, horizontal_depth, mx + (mw - w) / 2.0, my)
        }
        Edge::Bottom => {
            let w = horizontal_length(count, s).min(mw - 32.0);
            (w, horizontal_depth, mx + (mw - w) / 2.0, my + mh - horizontal_depth)
        }
    };
    // User nudge, then clamp so at least a grabbable strip stays on-screen.
    x = (x + offset_x.clamp(-400.0, 400.0)).clamp(mx - w + 80.0, mx + mw - 80.0);
    y = (y + offset_y.clamp(-400.0, 400.0)).clamp(my - h + 80.0, my + mh - 80.0);
    window.set_size(Size::Logical(LogicalSize::new(w, h))).map_err(|e| e.to_string())?;
    window.set_position(Position::Logical(LogicalPosition::new(x, y))).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn place_tooltip(app: &AppHandle, edge: Edge, index: usize, notch_scale: f64) -> Result<(), String> {
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

    let (x, y) = match edge {
        Edge::Right | Edge::Left => {
            let center = (CURL + START_PAD + index as f64 * (CELL + GAP) + RING / 2.0) * k;
            let y = (ny + center - TOOLTIP_H / 2.0).max(0.0);
            let x = match edge { Edge::Right => nx - TOOLTIP_W + 9.0, _ => nx + nw - 9.0 };
            (x, y)
        }
        Edge::Top | Edge::Bottom => {
            let center = (CURL + START_PAD + index as f64 * (HORIZONTAL_CELL + GAP) + RING / 2.0) * k;
            let x = (nx + center - TOOLTIP_W / 2.0).max(0.0);
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
    // Enforce size before centering so the window can never open half
    // off-screen from a stale size. Center twice: set_size applies
    // asynchronously, so the first center may still see the old size.
    window.center().map_err(|e| e.to_string())?;
    window.set_size(Size::Logical(LogicalSize::new(620.0, 660.0))).map_err(|e| e.to_string())?;
    window.center().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn place_context_menu(app: &AppHandle, edge: Edge, notch_scale: f64) -> Result<(), String> {
    let notch = app.get_webview_window("notch").ok_or("Notch window missing")?;    let menu = app.get_webview_window("context-menu").ok_or("Context menu window missing")?;
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

/// Is the OS cursor inside the notch or tooltip window right now?
///
/// Showing a top-level window can deliver a spurious `mouseleave` to the
/// notch webview even when the cursor never moved. The frontend asks this
/// before hiding the tooltip so a phantom leave can't blink it away.
/// Returns `None` when the position can't be determined (non-Windows or any
/// OS call fails) so the caller falls back to the plain timeout behavior.
pub fn cursor_inside_notch_or_tooltip(app: &AppHandle) -> Option<bool> {
    #[cfg(target_os = "windows")]
    {
        let (cx, cy) = cursor_position()?;
        for label in ["notch", "tooltip"] {
            let window = app.get_webview_window(label)?;
            let position = window.outer_position().ok()?;
            let size = window.outer_size().ok()?;
            // Skip hidden windows (outer rects are meaningless while hidden).
            if !window.is_visible().unwrap_or(false) {
                continue;
            }
            let (x, y) = (position.x, position.y);
            let (w, h) = (size.width as i32, size.height as i32);
            // Small forgiveness margin so edge pixels still count as inside.
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

#[cfg(target_os = "windows")]
fn cursor_position() -> Option<(i32, i32)> {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
    }
    let mut point = Point { x: 0, y: 0 };
    // SAFETY: GetCursorPos only writes the two ints through the pointer.
    let ok = unsafe { GetCursorPos(&mut point) };
    (ok != 0).then_some((point.x, point.y))
}

/// Frosted-glass notch. Windows acrylic needs no transparent tricks from us —
/// the CSS surface already carries the opacity. Best effort elsewhere.
pub fn set_notch_blur(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let window = app.get_webview_window("notch").ok_or("Notch window missing")?;
    if !enabled {
        window.set_effects(None).map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        use tauri::window::{Effect, EffectState, EffectsBuilder};
        let effects = EffectsBuilder::new()
            .effect(Effect::Acrylic)
            .state(EffectState::Active)
            .build();
        window.set_effects(Some(effects)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Resize the settings window to fit its content (measured frontend-side),
/// clamped to the monitor so it can neither scroll nor clip.
pub fn fit_settings(app: &AppHandle, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("settings")
        .ok_or("Settings window missing")?;
    let (_, _, _, mh) = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .as_ref()
        .map(monitor_rect)
        .or_else(|| primary_rect(app).ok())
        .ok_or("No monitor for settings window")?;
    let h = height.clamp(420.0, (mh - 60.0).max(420.0));
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let w = size.width as f64 / scale;
    window
        .set_size(Size::Logical(LogicalSize::new(w, h)))
        .map_err(|e| e.to_string())?;
    Ok(())
}
