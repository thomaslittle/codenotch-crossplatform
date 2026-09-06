# Plan: Auto-hide v1 — edge retract + hover peek

A trackable, resumable implementation plan. **Any agent (or human) must be able to pick
this up at any point — including mid-edit — and continue without re-deriving context.**

| | |
|---|---|
| **Feature** | Notch auto-hide: slides under the screen edge after an idle delay, peeks back out on hover at its resting spot |
| **Status** | Implementation complete through T5; verification/manual QA pending |
| **Last updated** | 2026-09-06 |
| **Working tree expectation** | Clean at `main` (`3cc3c4b`) when this plan was written |
| **Progress journal** | Bottom of this document — append an entry every working session |

---

## 0. How to use this doc

- Tasks are numbered `T0`–`T7` and are designed to be **small, independent, and each
  leaves the tree compiling**. Do them in order unless a dependency note says otherwise.
- Checkbox grammar: `☐` pending · `◐` in progress (must be resolved to `☑` or
  deliberately reverted before moving on — never build on a red tree) · `☑` done with
  its verification gate passing.
- **Do not re-litigate design decisions.** Section 5 records why each choice was made.
  If a decision turns out to be wrong in practice, stop, write up the finding in the
  journal, and update Sections 4–5 together.
- Anything discovered mid-task that contradicts this plan: fix the plan in the same
  session. A stale plan is a broken handoff.

## 1. Resume protocol (read this first when picking up mid-work)

1. **Assess the tree**: `git status` and `git diff`. Each task below lists the exact
   files it touches, and no two tasks touch the same file with overlapping intent —
   so the diff tells you which task was in flight. Untracked files also map to tasks
   (e.g. `src/lib/settings.test.ts` ⇒ T1).
2. **Read the progress journal** (bottom). The last entry states where the previous
   session stopped and what the next step was.
3. **Run the gates** to see if the tree is green:
   ```bash
   npm run typecheck
   npm test
   cargo check --manifest-path src-tauri/Cargo.toml
   ```
4. **If the in-flight task is incomplete**: finish it per its spec, or `git checkout --
   <its files>` to revert just that task. Then re-run the gates. Only proceed to the
   next `☐` task from a green tree.
5. **Before ending any session**: tick/adjust checkboxes, append a journal entry
   (date, tasks touched, exact state, next step). Leave work uncommitted unless the
   user explicitly asked for commits — the repo's etiquette is PRs reviewed by the
   maintainer, and committing is the user's call.

## 2. Background & goals

Codenotch is a Tauri 2 (Rust + React/TS) edge notch showing coding-assistant usage
(Claude Code, Cursor, Codex, Antigravity, OpenCode Zen). The notch is currently always
visible. The user wants classic auto-hide behavior:

- after some idle time, the notch **slides away under the screen edge**;
- moving the mouse to where it was **slides it back out**;
- after the mouse leaves again, it hides once more.

**Hard requirement surfaced during design:** multi-monitor correctness. Windows/Linux-X11
desktops are one big virtual space — "push the window off-screen" past monitor 1's right
edge *lands the window visibly on monitor 2's left edge*. So the hiding must never rely
on the window leaving the monitor. See Section 5, D1.

**Success criteria:**
1. Auto-hide works on all four edges and on any monitor, with zero pixels of the hidden
   notch appearing on any other monitor.
2. Hovering the resting spot reliably peeks the notch; the screen never strands the user
   (there is always a way to get the notch back without restarting).
3. When hidden, the notch's window does not swallow clicks on content beneath it.
4. Feature is opt-in (default off) and configurable (delay).
5. Existing behaviors are unaffected when auto-hide is off.

## 3. Non-goals & future work (do not build now)

- **Floating bubble mode** (collapse to a small draggable orb) — separate plan doc, needs
  product decisions (what it shows, drag semantics, click action).
- **Gauge themes** (speedometer/needle ring variants) — separate plan doc; the ring is
  already SVG so this is mostly presentation.
- **Fully-hidden (no sliver) mode** — possible later by increasing the translate
  distance; the poll-based hotspot already supports an invisible hotspot. V1 ships the
  visible 6px sliver for discoverability.
- Persisting retracted state across restarts (restart always starts visible — deliberate).

## 4. Design (decided)

### 4.1 Core mechanism — "the window is the overflow container"

- The notch **window's geometry never changes** for auto-hide. It stays exactly where
  `place_notch` put it — entirely within its monitor. This is what makes multi-monitor
  safe: the webview content is clipped at the window bounds, and those bounds coincide
  with the monitor edge, so the sliding content *cannot* render on another monitor.
  (This is the user's "container it slides under with overflow hidden" idea, with the
  native window as the container.)
- Retraction is a **CSS transform** on the notch shell (framer-motion `x`/`y`, same
  idiom as the existing entrance spring). The content slides toward the docked edge and
  gets clipped, leaving a visible **6 design-px sliver** of the shell's inner edge at the
  screen edge.
- Translate distances (design px, mirrored from `windows.rs` constants):
  - edge `right`: `x = +(SIDE_DEPTH − SLIVER) = +64`
  - edge `left`: `x = −64`
  - edge `top`: `y = −(HORIZONTAL_DEPTH − SLIVER) = −78`
  - edge `bottom`: `y = +78`
  - Constants: `SIDE_DEPTH = 70`, `HORIZONTAL_DEPTH = 84`, `SLIVER = 6`.

### 4.2 Input while retracted — click-through + cursor poll

- Transparent areas of a webview window still swallow mouse input
  (`docs/ARCHITECTURE.md`), so while retracted Rust calls
  `window.set_ignore_cursor_events(true)` → the whole (mostly transparent) window
  becomes click-through. Nothing under it is blocked.
- A click-through window receives no mouse events, so **Rust polls the cursor position**
  every **120 ms** while retracted and compares it against a **hotspot rect**: the band
  of the notch window's rect within `SLIVER * 2 = 12` logical px of the docked edge
  (full length of the window). Poll only runs while retracted — zero cost otherwise.
- Poll hit ⇒ flip state, `set_ignore_cursor_events(false)`, emit `"notch:peek"` to the
  `notch` window. The frontend springs the content back in.

### 4.3 State & event flow

```
visible ──(mouse leave shell + autoHideDelaySec, cursor not over tooltip/menu/settings)──▶ retracting
retracting ──(motion spring completes)──▶ invoke set_notch_retracted(true, edge)
                                          [Rust: ignore_cursor_events(true) + start poll]
hidden ──(poll: cursor enters hotspot)──▶ [Rust: ignore off, emit "notch:peek"]
          ──(frontend receives notch:peek)──▶ invoke set_notch_retracted(false, edge), then spring back in
```

Rules:
- The hide delay timer is **scheduled on shell `onMouseLeave`** and **cancelled on shell
  `onMouseEnter`**. When the delay fires, the frontend first asks Rust
  `cursor_over_overlay()` (tooltip / context-menu / settings windows) and postpones if
  the cursor is over any of them (e.g. the user is reading the hover card — notch stays
  out).
- Rust owns a single boolean (`AutohideState`); both the command and the poll mutate it.
  The command is idempotent (re-requesting the current state is a no-op).
- Any placement change (edge/scale/monitor/nudge/provider count) **un-retracts first**.
- `app_action "hide-hour"` emits `notch:peek` before hiding so the notch returns
  fully visible after the hour.
- Retracted state is **not** persisted; startup is always visible.
- Browser dev mode (`npm run dev`): auto-hide bridge functions no-op (`runningInTauri()`
  guards) — the notch simply never auto-hides in the browser demo.

### 4.4 New Rust surface

| Item | Kind | Notes |
|---|---|---|
| `AutohideState { retracted: AtomicBool }` | managed state | `app.manage` alongside `ProviderStore` |
| `set_notch_retracted(retracted: bool, edge: Edge)` | command | idempotent; flips `set_ignore_cursor_events`, spawns/stops poll |
| `cursor_over_overlay()` | command | `Option<bool>` — cursor over tooltip/context-menu/settings; mirrors existing `cursor_overtooltip_area` |
| `autohide_supported()` | command | Windows: `true`. Linux: Xlib display opens. Used to disable the toggle honestly |
| `spawn_retract_poll(app, edge)` | internal | 120 ms loop; exits when flag false, window hidden, or hotspot hit |
| `cursor_position_global()` | internal | Windows: existing `GetCursorPos` FFI. Linux: Xlib `XQueryPointer` FFI with a cached `Display*` |
| `hotspot_rect(window_rect, edge, depth)` | internal, pure | band along docked edge; unit-testable |

No `capabilities/default.json` changes are needed — custom app commands don't require
capability entries (existing commands prove the pattern).

### 4.5 New frontend surface

| Item | File | Notes |
|---|---|---|
| `autoHide: boolean`, `autoHideDelaySec: number` | `types.ts`, `settings.ts` | defaults `false`, `5`; clamp delay to 1–60; UI presets 2/5/10/30 |
| `setNotchRetracted(retracted, edge)` | `lib/backend.ts` | no-op when not in Tauri |
| `cursorOverOverlay()` | `lib/backend.ts` | returns `boolean \| null` (null = unknown) |
| `autohideSupported()` | `lib/backend.ts` | returns `false` when not in Tauri |
| retracted state + spring + timers + `notch:peek` listener | `views/NotchView.tsx` | both branches (provider stack *and* empty state) |
| "Auto-hide" settings section | `views/SettingsView.tsx` | toggle + delay select; disabled with a note when unsupported |
| unit tests for settings clamping | `lib/settings.test.ts` (new) | follow `usage.test.ts` style (vitest) |

### 4.6 Known risks (verify, don't assume)

- **R1 — `zoom` × transform interaction.** The shell applies CSS `zoom: settings.scale`
  on the same element motion animates. Chrome is expected to scale the rendered
  translation by the zoom factor (so a 64 px translate covers `64 × scale` of the real
  viewport, matching the window size Rust computed). **T0 verifies this empirically.**
  Fallback if wrong: multiply the translate distance by `settings.scale` manually.
- **R2 — `onAnimationComplete` races.** Rapid hide→peek→hide sequences can complete
  animations out of order. Guard with a generation counter (the file already uses this
  pattern for `leaveGen`).
- **R3 — reduced motion.** `MotionConfig reducedMotion="user"` makes springs instant;
  `onAnimationComplete` is expected to still fire. T4 verifies; if it doesn't, trigger
  the Rust snap from a `setTimeout` fallback of ~300 ms.
- **R4 — Linux Xlib.** `XOpenDisplay`/`XQueryPointer` FFI with a cached display handle;
  any failure ⇒ `autohide_supported() = false` ⇒ the toggle disables itself. Never
  retract on a machine that cannot peek (that would strand the notch).
- **R5 — hotspot vs. adjacent monitor.** With monitor 2 docked to the notch's edge, the
  cursor crosses the boundary and leaves the hotspot; that's fine — peek already
  triggered on entry, and normal `mouseleave` re-hide logic takes over after peek.

## 5. Decisions log (do not re-litigate; update only with evidence)

- **D1 — Window never moves for auto-hide.** Moving it off-screen breaks on multi-monitor
  (virtual-desktop geometry lands it on the adjacent monitor). Per-frame window moves
  also can't hold 60 fps (`windows.rs` placement comment). Window-clip + CSS transform is
  the only approach that is simultaneously smooth, multi-monitor-correct, and idiomatic
  to this codebase.
- **D2 — Sliver stays visible (6 design px).** Discoverability + unambiguous hover
  target. A fully-hidden variant is a later toggle (Section 3).
- **D3 — Cursor polling, not mouse hooks.** 120 ms polling is simple, dependency-free,
  reuses the existing `GetCursorPos` FFI style, and is imperceptible for a peek gesture.
  A low-level mouse hook (`SetWindowsHookEx`) is the upgrade path if latency ever matters.
- **D4 — `set_ignore_cursor_events(true)` while hidden.** Required because transparent
  webview bounds swallow clicks (documented in `docs/ARCHITECTURE.md`); without it the
  hidden notch blocks maximized windows' scrollbars at the screen edge.
- **D5 — Opt-in, default off.** Existing users' behavior is unchanged; the maintainer can
  flip the default later.
- **D6 — Overlay-check before retracting** (tooltip/context-menu/settings windows).
  Prevents the notch sliding away while the user is reading the detail card or using a
  menu — matches the app's existing "honest, never surprising" interaction philosophy.

## 6. Key file map

```
src/types.ts                     ClientSettings shape
src/lib/settings.ts              settings load/save/clamp (localStorage)
src/lib/backend.ts               Tauri invoke wrappers (runningInTauri guards, throttling)
src/views/NotchView.tsx          notch UI, hover/leave timers, motion springs
src/views/SettingsView.tsx       settings sections (rise-animated <section class="settings-section">)
src/styles.css                   .notch-shell & edge shaping (curl via ::before/::after)
src-tauri/src/windows.rs         Edge enum, placement, GetCursorPos FFI, cursor rect checks
src-tauri/src/lib.rs             commands, managed state, window events, app_action
src-tauri/src/model.rs           ProviderStore (pattern reference for managed state)
docs/plans/autohide-v1.md        this document
```

Verification gates used throughout:

```bash
npm run typecheck                # TS
npm test                         # vitest
cargo check --manifest-path src-tauri/Cargo.toml   # fast Rust gate
cargo test --manifest-path src-tauri/Cargo.toml    # full Rust gate
npm run build                    # production bundle
npm run tauri dev                # manual QA (real app)
```

## 7. Codebase conventions to follow

- Frontend bridge functions early-return when `!runningInTauri()`.
- Rust commands return `Result<(), String>` with `.map_err(|e| e.to_string())`.
- Motion springs: stiffness ~210–480, damping ~26–32 (match neighbors).
- Comments explain *constraints*, not narration (see existing `windows.rs` comments).
- Honest statuses over invented behavior (project philosophy — applies to
  `autohide_supported` too: disable the feature rather than fake it).
- No new npm dependencies; Rust additions prefer std + existing crates (Xlib FFI is
  hand-rolled, mirroring the `user32` FFI style already in `windows.rs`).

---

## 8. Tasks

### T0 — ◐ Verify zoom × transform assumption (R1) — *decision resolved; Tauri spot-check pending*

**Files:** none permanent (scratch edit + revert, or a temporary branch).
**Spec:** In `NotchView.tsx`, temporarily hard-code the shell's `animate` to
`x: 64` with edge `right` and `npm run tauri dev` at scale 100%, then 70%, then 130%
(Settings → Notch size). Measure whether the visible gap between the shell's inner edge
and the screen edge equals `64 × scale` (zoom scales transforms) or stays `64`
(it doesn't).
**Acceptance:** Journal entry records the observed behavior at all three scales and the
decision: translate values are design px (zoom scales) or design px × scale (manual).
T4 consumes this decision.

### T1 — ◐ Settings model — *implemented; gates pending*

**Files:** `src/types.ts`, `src/lib/settings.ts`, `src/lib/settings.test.ts` (new).
**Spec:**
- `ClientSettings` += `autoHide: boolean` (default `false`), `autoHideDelaySec: number`
  (default `5`), with doc comments matching the file's style.
- `DEFAULT_SETTINGS` += both fields.
- `loadSettings()`: `autoHide = typeof parsed.autoHide === "boolean" ? … : default`;
  `autoHideDelaySec` clamped 1–60 via the existing `clampNumber` helper.
- New `settings.test.ts`: defaults when storage empty; clamping out-of-range values;
  legacy payloads without the new fields; boolean coercion (e.g. `"true"` string must
  NOT count as true). Mirror the import/setup style of `src/lib/usage.test.ts`.
**Acceptance:** `npm run typecheck && npm test` green.

### T2 — ◐ Frontend bridge functions — *implemented; gate pending*

**Files:** `src/lib/backend.ts`.
**Spec:** Add three wrappers, each with the standard `if (!runningInTauri())` early
return (`setNotchRetracted`/`cursorOverOverlay` no-op, `autohideSupported` returns
`false`):
```ts
export async function setNotchRetracted(retracted: boolean, edge: Edge): Promise<void>
  // invoke("set_notch_retracted", { retracted, edge }); swallow errors like hideTooltip peers? No — let caller decide: catch and console.error, do not throw.
export async function cursorOverOverlay(): Promise<boolean | null>
  // invoke<boolean | null>("cursor_over_overlay"); catch → null (same contract as cursorOverTooltipArea)
export async function autohideSupported(): Promise<boolean>
  // invoke<boolean>("autohide_supported"); catch → false
```
**Acceptance:** `npm run typecheck` green. (Functions unused until T4/T5 — that's fine;
do not wire them here.)

### T3 — ◐ Rust: state, commands, cursor read, hotspot, poll — *implemented; gates pending*

**Files:** `src-tauri/src/windows.rs`, `src-tauri/src/lib.rs`.
**Spec:**
- `windows.rs`:
  - `pub const SLIVER: f64 = 6.0;`
  - Generalize `cursor_position()` → `pub fn cursor_position_global() -> Option<(i32, i32)>`:
    keep the Windows `GetCursorPos` body; add `#[cfg(target_os = "linux")]` Xlib FFI
    (`XOpenDisplay(null)`, `XDefaultRootWindow`, `XQueryPointer`; cache the `Display*`
    in a `OnceLock`; close on drop or leak deliberately — document the choice). Return
    `None` on any failure.
  - `pub fn hotspot_rect(x: i32, y: i32, w: i32, h: i32, edge: Edge, depth_phys: i32)
    -> (i32, i32, i32, i32)` — pure; band along the docked edge. Unit-test all four
    edges (the file currently has no tests; add `#[cfg(test)] mod tests`).
  - `pub fn autohide_supported() -> bool` (Windows `true`; Linux: probe the Xlib display
    once).
  - `pub fn set_notch_retracted(app: &AppHandle, retracted: bool, edge: Edge) -> Result<(), String>`
    — idempotent against the state flag; `window.set_ignore_cursor_events(retracted)`;
    on `true` spawn the poll, on `false` the poll self-terminates via the flag.
  - Poll loop (spawned via `tauri::async_runtime::spawn`): every 120 ms — exit if state
    flag false; skip if `notch` window missing or `!is_visible()`; read cursor; if
    inside `hotspot_rect(notch.outer_position/size, edge, (SLIVER*2 * scale_factor) as i32)`
    ⇒ set flag false, `set_ignore_cursor_events(false)`, `app.emit_to("notch", "notch:peek", ())`,
    exit.
- `lib.rs`:
  - `struct AutohideState { retracted: std::sync::atomic::AtomicBool }`; `app.manage`.
  - Commands `set_notch_retracted(retracted: bool, edge: Edge)`,
    `cursor_over_overlay()` (reuse the rect-check pattern of
    `cursor_inside_notch_or_tooltip` but over `["tooltip", "context-menu", "settings"]`,
    leaving that existing function untouched), `autohide_supported()`. Register all
    three in `generate_handler!`.
  - `app_action "hide-hour"`: before hiding, emit `notch:peek` to the notch window so
    the webview un-retracts (notch returns fully visible after the hour).
**Acceptance:** `cargo check` + `cargo test` green (hotspot unit tests pass). Frontend
still typechecks (`npm run typecheck`) — commands unused so far.

### T4 — ◐ NotchView wiring — *implemented; typecheck + manual Tauri QA pending*

**Files:** `src/views/NotchView.tsx`. **Depends on:** T0 (translate decision), T1–T3.
**Spec:**
- Extract the shared `motion.main` props (the two return branches currently duplicate
  them) into a local const so both the provider-stack shell and the empty-state shell
  get auto-hide.
- `const [retracted, setRetracted] = useState(false)` + a generation ref
  (`retractGen`) guarding `onAnimationComplete` against out-of-order races (R2).
- Shell `animate` targets: `x/y = retracted ? hideOffset : 0` where `hideOffset` comes
  from the T0 decision (per-edge values in §4.1).
- `onMouseEnter` (shell) → cancel pending hide timer, bump `retractGen`;
  `onMouseLeave` (shell) → keep existing `setHoveredId(null)` AND schedule the hide
  timer: after `settings.autoHideDelaySec * 1000`, `cursorOverOverlay()`; if `true`
  re-schedule (poll again in 1 s), else retract.
- `retract()`: bump gen, `setRetracted(true)`; `onAnimationComplete` (only if gen still
  current and retracted) → `setNotchRetracted(true, edge)`.
- `peek()`: bump gen, `await setNotchRetracted(false, edge)` **then** `setRetracted(false)`
  (click-through must be off before the spring so mouse events flow immediately).
- `listen("notch:peek", peek)` alongside the existing listeners.
- The placement effect (existing `useEffect` on edge/scale/monitor/offsets/count):
  un-retract first (peek() or a lightweight reset) so placement changes always restore
  the full notch.
- Only engage when `settings.autoHide`; if `setNotchRetracted(true, …)` rejects
  (unsupported platform), immediately peek() and leave the notch visible.
**Acceptance:** `npm run typecheck` green; manual `npm run tauri dev`:
retract + peek works on `right` edge at 100% scale; journal note for R2/R3 outcomes.

### T5 — ◐ Settings UI — *implemented; typecheck/build/manual persistence QA pending*

**Files:** `src/views/SettingsView.tsx`. **Depends on:** T1, T2.
**Spec:**
- New `<section class="settings-section">` between **Appearance** and **Updates**:
  `<h2>Auto-hide</h2>`, following the file's existing rise-animated section pattern.
- Row 1: checkbox styled like the provider rows — "Auto-hide when idle".
- Row 2 (visible when enabled): delay `<select>` (classes as `monitor-select`) with
  presets 2/5/10/30 seconds, bound to `autoHideDelaySec` via `update(...)`.
- On mount, `autohideSupported()`; if `false`, render the section's controls `disabled`
  with a one-line honest note ("Needs cursor position access — unavailable on this
  system"), matching the app's tone in "How readings work".
**Acceptance:** `npm run typecheck && npm run build` green; settings window still
auto-fits (`fitSettings` picks up the new section — the 1 s poll handles it); toggling
persists via localStorage and reaches the notch through the existing
`settings:changed` event.

### T6 — ☐ Full manual QA matrix (Section 9) + fixes

**Files:** whatever the findings touch.
**Acceptance:** every matrix row recorded in the journal with pass/fail; failures
fixed or converted into follow-up plan tasks.

### T7 — ☐ Docs & wrap-up

**Files:** `README.md` ("What we added on top" bullet + the right-click/settings list),
this plan (final checkbox sweep, journal closing entry).
**Acceptance:** README accurate (default off, opt-in); plan fully ticked; tree green on
all four gates (Section 6).

## 9. QA matrix (run against `npm run tauri dev`, Windows primary)

| # | Scenario | Expected |
|---|---|---|
| 1 | All four edges: hide → peek → re-hide | Sliver visible at docked edge; spring in/out; no visual pop at the clip boundary |
| 2 | **Monitor 2 adjacent to the notch's edge; notch retracts** | Zero notch pixels on monitor 2 (the core regression test) |
| 3 | Notch placed on monitor 2 (selector) | Auto-hide + peek work; hotspot on monitor 2's edge |
| 4 | Scale 70% and 130% | Sliver scales with the notch; peek still triggers; translate distance correct (T0 decision holds) |
| 5 | Opacity 40% | Sliver still visible enough to find; no input regression |
| 6 | Delay behavior | Enter cancels timer; leave hides after delay; hovering the detail card holds the notch out; closing card → hides |
| 7 | Context menu open, cursor over it | No retract while cursor over the menu |
| 8 | Settings window overlapping the notch, cursor over settings | No retract while cursor over settings |
| 9 | Right-click → Hide for 1 hour | Returns after the hour fully visible (not slivered) |
| 10 | All providers disabled (empty state) | Auto-hide works on the orb-only shell; peek restores it |
| 11 | App restart while hidden | Notch starts visible (state not persisted) |
| 12 | OS reduced motion enabled | Hide/show instant; never stranded hidden |
| 13 | Browser dev mode (`npm run dev`) | No auto-hide activity, no console errors |
| 14 | Auto-hide toggle off | Behavior identical to current `main` |
| 15 | Under-notch input while hidden | Clicks pass through the hidden strip (e.g. a maximized window's scrollbar at the edge) |
| 16 | Linux/X11 (if available; otherwise mark "code-reviewed only" honestly) | Peek poll works; `autohide_supported` gate honest |

## 10. Definition of done

- [ ] T0–T7 all `☑`, journal complete
- [ ] All four gates green (Section 6)
- [ ] QA matrix rows 1–15 pass on Windows; row 16 best-effort with honest status
- [ ] Multi-monitor invariant demonstrated (row 2)
- [ ] README updated; feature is opt-in and documented

## Progress journal (append-only; newest last)

- **2026-09-06 — plan created** (session: familiarization + design). Codebase surveyed;
  no existing auto-hide or roadmap mention found. Design settled on window-clip +
  click-through + cursor poll after rejecting window-move (multi-monitor breakage).
  All tasks pending; tree clean at `3cc3c4b`.
- **2026-09-06 — implementation session** (branch `feat/autohide-v1`). Picked up from
  plan commit `0ebed25`. T0's core CSS question was measured in a real Chromium layout:
  `translateX(64px)` produced physical deltas of `64px` at 100% zoom, `44.8px` at 70%,
  and `83.2px` at 130%. Decision: use the §4.1 design-pixel offsets directly; CSS zoom
  already scales the transform, so multiplying by `settings.scale` would double-scale.
  A final Windows Tauri/WebView2 spot-check remains part of T4/T6 QA.
- **2026-09-06 — T1–T5 code implemented**. Added settings/defaults/clamping + Vitest
  coverage; frontend bridge wrappers; native `AutohideState`, Windows/X11 cursor reads,
  four-edge hotspot tests, click-through + 120ms peek poll and commands; shared
  `NotchView` retract/peek wiring with generation guards and placement reset; and the
  opt-in Auto-hide settings UI with 2/5/10/30s presets and support gating. Branch head
  after feature code: `0dd92aafac337715833b46cc60c3f1944fd846c3`. Diff against base is
  limited to this plan and the T1–T5 files; no unrelated files were changed.
- **2026-09-06 — verification status / next step**. This session's execution container
  cannot fetch the GitHub checkout directly and has no Rust toolchain, while repo CI only
  runs on `main` pushes or pull requests. Therefore **no npm/cargo/Tauri gate is claimed
  green here**. T1–T5 stay `◐` until a real checkout runs `npm run typecheck`, `npm test`,
  `npm run build`, `cargo check --manifest-path src-tauri/Cargo.toml`, and
  `cargo test --manifest-path src-tauri/Cargo.toml`. Then run `npm run tauri dev` and
  complete T4's right-edge smoke test plus the full Section 9 matrix (especially R2/R3,
  reduced motion, click-through, and the adjacent-monitor invariant) before T7 docs.
