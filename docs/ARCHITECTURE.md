# Architecture

## Why this is a rewrite, not a Swift port

The reference application is built around SwiftUI, AppKit panels, macOS Keychain, `NSPanel`, macOS screen APIs, and a macOS updater. Those are the exact pieces that prevent it from running on Windows/Linux. Keeping that architecture and wrapping it would preserve the platform lock-in.

This implementation instead keeps the product contract and separates it into four portable layers:

1. **React presentation** — rings, tooltip, settings, transitions, accessibility.
2. **Tauri window shell** — tiny native top-level windows with no taskbar entry.
3. **Rust provider adapters** — file/SQLite/network readers that never expose credentials to the webview.
4. **Platform placement** — Windows/X11-compatible absolute positioning isolated in `windows.rs`.

## Window topology

The app intentionally uses four windows:

- `notch` — only the visible edge surface.
- `tooltip` — shown beside the hovered provider.
- `settings` — centered configuration sheet.
- `context-menu` — compact right-click surface.

A single huge transparent window would be simpler visually but would create hit-testing and focus problems because transparent webview bounds still participate in desktop input. Small windows preserve normal desktop interactions everywhere else.

## Provider boundary

Provider adapters return one normalized shape:

```text
ProviderSnapshot
  id / displayName / glyph
  fidelity / status / fetchedAt
  account
  windows[] -> id / label / usedFraction / resetsAt
  headlineId
  activity
```

The UI does not need to know whether a number came from JSONL, SQLite + HTTP, or an OAuth endpoint.

## Failure behavior

Adapters return explicit error states instead of guessed percentages. `ProviderStore` keeps the last successful snapshot in memory; if the next fetch fails, that previous snapshot becomes `stale` and the UI dims it. This is especially important for unofficial/internal usage endpoints that can rate-limit or change shape.
