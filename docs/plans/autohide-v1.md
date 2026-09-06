# Auto-hide v1 — completion record

## Status

**Complete — 2026-09-06**

Auto-hide v1 is implemented on `feat/autohide-v1` and covered by PR #1.
The feature is intentionally limited to classic edge auto-hide behavior; layout,
theming, floating-orb ideas, and fully-hidden variants are separate future features.

## Shipped behavior

- Auto-hide is opt-in and defaults to off.
- Configurable idle delay with 2 / 5 / 10 / 30 second presets.
- Works on right, left, top, and bottom screen edges.
- Retraction leaves a 6 design-pixel discovery sliver.
- Hovering the resting edge peeks the notch back out.
- The native notch window never moves outside its monitor while retracting.
- Hidden content is clipped inside the existing native window, so an adjacent monitor
  cannot receive the retracted notch pixels.
- Native click-through is enabled while retracted so transparent notch bounds do not
  swallow clicks or scrolling underneath.
- A 120 ms native cursor poll restores the notch because a click-through window cannot
  receive normal mouse events.
- Windows uses `GetCursorPos`.
- Linux/X11 uses `XQueryPointer`; unsupported environments disable the setting rather
  than risking a stranded hidden notch.
- Tooltip, context-menu, and Settings overlays hold the notch open while in use.
- Closing Settings explicitly re-arms the idle timer.
- Placement changes (edge, scale, monitor, offsets, provider count) restore the notch
  before repositioning.
- Empty-provider/settings-only mode uses the same auto-hide path.
- Retracted state is not persisted; restart begins visible.
- Existing behavior is unchanged when auto-hide is disabled.

## Scaling decision

The notch already has a 70%–130% visual scaling control. Chromium testing established
that CSS `zoom` scales transforms on the same element, so the auto-hide offsets remain
raw design pixels rather than being multiplied by `settings.scale` a second time.

Measured `translateX(64px)` results:

- 100% scale: 64 px
- 70% scale: 44.8 px
- 130% scale: 83.2 px

Windows desktop testing confirmed auto-hide behaves correctly across the existing scale
range.

## Native design

The native window is the overflow container. Auto-hide changes only the webview content
transform; it does **not** push the native window beyond the monitor edge.

Design offsets:

- right: `x = +64`
- left: `x = -64`
- top: `y = -78`
- bottom: `y = +78`

Constants:

- side depth: 70
- horizontal depth: 84
- visible sliver: 6

While fully retracted, Rust calls `set_ignore_cursor_events(true)`. A bounded native
cursor poll watches a hotspot along the docked edge. Entering that hotspot restores
input first and emits `notch:peek`, after which the frontend springs the content back
into view.

## Implementation map

- `src/types.ts` — auto-hide settings shape
- `src/lib/settings.ts` — defaults, persistence, delay clamping
- `src/lib/settings.test.ts` — persistence/clamping/legacy coverage
- `src/lib/backend.ts` — Tauri auto-hide bridges
- `src/views/NotchView.tsx` — timers, retract/peek state, races, overlay behavior
- `src/views/SettingsView.tsx` — settings UI, Settings-close re-arm, resize/drag fixes
- `src-tauri/src/lib.rs` — managed state and commands
- `src-tauri/src/windows.rs` — cursor reads, hotspot tests, click-through and polling
- `src-tauri/capabilities/default.json` — narrow window permissions needed by the
  Settings resize/recenter fix (`allow-center` and `allow-start-dragging`)
- `README.md` — user-facing auto-hide documentation

## Verification

PR CI was run on the final code path on both supported CI platforms.

### Windows

Passed:

- `npm run typecheck`
- `npm test`
- `npm run build`
- Rust tests
- full `npm run tauri build`

### Ubuntu 24.04

Passed:

- `npm run typecheck`
- `npm test`
- `npm run build`
- Rust tests, including the X11-linked path and four-edge hotspot tests
- full `npm run tauri build`

## Manual Windows acceptance

The feature was exercised in `npm run tauri dev`. The user confirmed the implementation
behaves correctly, including the highest-risk cases:

- hide → peek → re-hide on all four edges
- adjacent-monitor clipping invariant
- notch operation on multi-monitor setups
- 70% and 130% scaling
- native click-through while hidden
- Settings interaction after the fixes below

Two issues were found during QA and fixed before completion:

1. **Auto-hide did not re-arm after closing Settings.**
   The notch had no new `mouseleave` event after enabling auto-hide while Settings was
   open. Settings now emits `settings:closed`, and the notch explicitly schedules its
   configured idle timer.
2. **Settings dragging/resizing was unreliable.**
   Tauri drag-region attributes are now present on the header text itself, and content
   height changes re-center the native Settings window so enabling Auto-hide cannot push
   it below the screen. Required narrow window capabilities were added.

The user accepted the current Windows behavior as complete and will report any additional
runtime bugs if encountered. Such bugs can be fixed against this feature as appropriate;
new product/layout/theming ideas should not expand this PR's scope.

## Non-goals / separate future features

These are deliberately **not** missing Auto-hide v1 work:

- floating/draggable bubble or orb mode
- fully-hidden mode with no discovery sliver
- alternate gauge/ring themes
- broader notch layout redesign
- expanded visual scaling range beyond the existing control
- persisting the retracted state across restart

Each should receive its own plan/branch if pursued.

## Definition of done

- [x] Settings model, persistence, clamping, and tests
- [x] Frontend Tauri bridges
- [x] Windows and Linux/X11 native cursor support
- [x] Four-edge hotspot logic and tests
- [x] Native click-through + bounded retract polling
- [x] Notch retract/peek wiring and race guards
- [x] Auto-hide Settings UI
- [x] Settings-close re-arm bug fixed
- [x] Settings drag/recenter bug fixed
- [x] Multi-monitor invariant manually accepted on Windows
- [x] Existing 70%–130% scaling manually accepted
- [x] Hidden input click-through manually accepted
- [x] README updated
- [x] Windows CI green
- [x] Ubuntu CI green
- [x] Feature scope frozen; future layout/theming work separated

Auto-hide v1 is complete and ready for maintainer review.
