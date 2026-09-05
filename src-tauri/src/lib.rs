mod model;
mod providers;
mod windows;

use model::ProviderSnapshot;
use providers::ProviderStore;
use tauri::{Emitter, Manager, State, WindowEvent};
use windows::Edge;

#[tauri::command]
async fn get_snapshots(store: State<'_, ProviderStore>) -> Result<Vec<ProviderSnapshot>, String> {
    Ok(store.snapshots().await)
}

#[tauri::command]
fn set_edge(app: tauri::AppHandle, edge: Edge, provider_count: usize) -> Result<(), String> {
    windows::place_notch(&app, edge, provider_count)
}

#[tauri::command]
fn show_tooltip(app: tauri::AppHandle, edge: Edge, index: usize) -> Result<(), String> {
    windows::place_tooltip(&app, edge, index)
}

#[tauri::command]
fn hide_window(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(&label) {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    windows::open_settings(&app)
}

#[tauri::command]
fn show_context_menu(app: tauri::AppHandle, edge: Edge) -> Result<(), String> {
    windows::place_context_menu(&app, edge)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    const ALLOWED: [&str; 4] = ["https://claude.ai/", "https://cursor.com/", "https://chatgpt.com/", "https://antigravity.google/"];
    if !ALLOWED.iter().any(|prefix| url.starts_with(prefix)) {
        return Err("URL is not an allowed provider destination".into());
    }
    open::that(url).map_err(|e| e.to_string())
}

#[tauri::command]
fn app_action(app: tauri::AppHandle, action: String) -> Result<(), String> {
    match action.as_str() {
        "refresh" => app.emit_to("notch", "app:refresh", ()).map_err(|e| e.to_string()),
        "settings" => windows::open_settings(&app),
        "quit" => { app.exit(0); Ok(()) }
        "hide-hour" => {
            if let Some(notch) = app.get_webview_window("notch") { notch.hide().map_err(|e| e.to_string())?; }
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(60 * 60)).await;
                if let Some(notch) = handle.get_webview_window("notch") { let _ = notch.show(); }
            });
            Ok(())
        }
        _ => Err(format!("Unknown app action: {action}")),
    }
}

pub fn run() {
    #[cfg(target_os = "linux")]
    {
        // Absolute edge positioning is deterministic on X11/XWayland. Native
        // Wayland compositors intentionally restrict arbitrary global window
        // positioning, so prefer XWayland when a DISPLAY bridge is present.
        if std::env::var_os("DISPLAY").is_some() && std::env::var_os("GDK_BACKEND").is_none() {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    tauri::Builder::default()
        .manage(ProviderStore::default())
        .invoke_handler(tauri::generate_handler![
            get_snapshots, set_edge, show_tooltip, hide_window, open_settings,
            show_context_menu, app_action, open_url
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "notch" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("notch") {
                let _ = window.set_always_on_top(true);
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                let _ = windows::place_notch(&handle, Edge::Right, 4);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Codenotch");
}
