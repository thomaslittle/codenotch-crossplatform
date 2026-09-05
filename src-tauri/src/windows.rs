use serde::Deserialize;
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

fn monitor_logical(app: &AppHandle) -> Result<(f64, f64, f64, f64), String> {
    let monitor = app.primary_monitor().map_err(|e| e.to_string())?.ok_or("No primary monitor")?;
    let scale = monitor.scale_factor();
    let position = monitor.position();
    let size = monitor.size();
    Ok((
        position.x as f64 / scale,
        position.y as f64 / scale,
        size.width as f64 / scale,
        size.height as f64 / scale,
    ))
}

fn side_length(count: usize) -> f64 {
    let count = count.max(1) as f64;
    CURL * 2.0 + START_PAD + END_PAD + count * CELL + (count - 1.0) * GAP
}

fn horizontal_length(count: usize) -> f64 {
    let count = count.max(1) as f64;
    CURL * 2.0 + START_PAD + END_PAD + count * HORIZONTAL_CELL + (count - 1.0) * GAP
}

pub fn place_notch(app: &AppHandle, edge: Edge, count: usize) -> Result<(), String> {
    let window = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let (mx, my, mw, mh) = monitor_logical(app)?;
    let (w, h, x, y) = match edge {
        Edge::Right => {
            let h = side_length(count).min(mh - 32.0);
            (SIDE_DEPTH, h, mx + mw - SIDE_DEPTH, my + (mh - h) / 2.0)
        }
        Edge::Left => {
            let h = side_length(count).min(mh - 32.0);
            (SIDE_DEPTH, h, mx, my + (mh - h) / 2.0)
        }
        Edge::Top => {
            let w = horizontal_length(count).min(mw - 32.0);
            (w, HORIZONTAL_DEPTH, mx + (mw - w) / 2.0, my)
        }
        Edge::Bottom => {
            let w = horizontal_length(count).min(mw - 32.0);
            (w, HORIZONTAL_DEPTH, mx + (mw - w) / 2.0, my + mh - HORIZONTAL_DEPTH)
        }
    };
    window.set_size(Size::Logical(LogicalSize::new(w, h))).map_err(|e| e.to_string())?;
    window.set_position(Position::Logical(LogicalPosition::new(x, y))).map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn place_tooltip(app: &AppHandle, edge: Edge, index: usize) -> Result<(), String> {
    let notch = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let tooltip = app.get_webview_window("tooltip").ok_or("Tooltip window missing")?;
    let scale = notch.scale_factor().map_err(|e| e.to_string())?;
    let p = notch.outer_position().map_err(|e| e.to_string())?;
    let s = notch.outer_size().map_err(|e| e.to_string())?;
    let nx = p.x as f64 / scale;
    let ny = p.y as f64 / scale;
    let nw = s.width as f64 / scale;
    let nh = s.height as f64 / scale;
    tooltip.set_size(Size::Logical(LogicalSize::new(TOOLTIP_W, TOOLTIP_H))).map_err(|e| e.to_string())?;

    let (x, y) = match edge {
        Edge::Right | Edge::Left => {
            let center = CURL + START_PAD + index as f64 * (CELL + GAP) + RING / 2.0;
            let y = (ny + center - TOOLTIP_H / 2.0).max(0.0);
            let x = match edge { Edge::Right => nx - TOOLTIP_W + 9.0, _ => nx + nw - 9.0 };
            (x, y)
        }
        Edge::Top | Edge::Bottom => {
            let center = CURL + START_PAD + index as f64 * (HORIZONTAL_CELL + GAP) + RING / 2.0;
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
    window.center().map_err(|e| e.to_string())?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn place_context_menu(app: &AppHandle, edge: Edge) -> Result<(), String> {
    let notch = app.get_webview_window("notch").ok_or("Notch window missing")?;
    let menu = app.get_webview_window("context-menu").ok_or("Context menu window missing")?;
    let scale = notch.scale_factor().map_err(|e| e.to_string())?;
    let p = notch.outer_position().map_err(|e| e.to_string())?;
    let s = notch.outer_size().map_err(|e| e.to_string())?;
    let nx = p.x as f64 / scale;
    let ny = p.y as f64 / scale;
    let nw = s.width as f64 / scale;
    let nh = s.height as f64 / scale;
    let (x, y) = match edge {
        Edge::Right => (nx - 194.0, ny + nh / 2.0 - 82.0),
        Edge::Left => (nx + nw - 6.0, ny + nh / 2.0 - 82.0),
        Edge::Top => (nx + nw / 2.0 - 90.0, ny + nh - 6.0),
        Edge::Bottom => (nx + nw / 2.0 - 90.0, ny - 158.0),
    };
    menu.set_position(Position::Logical(LogicalPosition::new(x, y))).map_err(|e| e.to_string())?;
    menu.show().map_err(|e| e.to_string())?;
    menu.set_focus().ok();
    Ok(())
}
