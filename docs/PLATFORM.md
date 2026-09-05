# Platform notes

## Windows

The Tauri window is frameless, transparent, always on top, non-resizable, and excluded from the taskbar. Edge placement uses the primary monitor coordinates reported by Tauri. Cursor state resolves through the roaming config directory; Codex and Claude use their home/config directories.

The app does not use Windows Credential Manager for Claude because current Claude Code Windows/Linux installations use file-based secure-storage/config credentials rather than the macOS Keychain service used by the reference application.

## Linux

### X11 / XWayland

This is the supported placement path for v0.1. When `DISPLAY` exists and `GDK_BACKEND` is not already selected, the Rust entrypoint selects `x11`. That produces deterministic edge coordinates on X11 and XWayland desktops.

### Native Wayland

Native Wayland intentionally does not expose the same absolute-positioning contract as X11/Win32. Correct desktop-edge behavior generally requires a layer-shell style protocol and compositor cooperation. The architecture keeps platform placement isolated so such a backend can be added without changing the provider or React layers.

## Multi-monitor

v0.1 anchors to the primary monitor. The window manager is deliberately isolated so a future settings option can select a monitor and respond to topology changes without touching provider/UI code.
