import { emitTo } from "@tauri-apps/api/event";
import { motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { autohideSupported, REPO_URL, fitSettings, getSnapshots, hideWindow, listMonitors, openExternal, runningInTauri } from "../lib/backend";
import { DEFAULT_SETTINGS, loadSettings, saveSettings } from "../lib/settings";
import { DARK_SURFACE, LIGHT_SURFACE, surfaceLuminance, themeVars, useSystemLight } from "../lib/theme";
import { checkForUpdates, type UpdateInfo } from "../lib/updates";
import { ProviderLogo } from "../components/ProviderLogo";
import type { ClientSettings, Edge, MonitorInfo, ProviderSnapshot, ThemeMode } from "../types";

const edges: Edge[] = ["right", "left", "top", "bottom"];
const modes: ThemeMode[] = ["dark", "light", "system"];
const autoHideDelays = [2, 5, 10, 30];

const rise = {
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
};

export function SettingsView() {
  const [settings, setSettings] = useState<ClientSettings>(loadSettings);
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [autoHideAvailable, setAutoHideAvailable] = useState<boolean | null>(null);
  const sysLight = useSystemLight();
  const activeMonitor = monitors.find((m) => m.id === settings.monitor)
    ?? monitors.find((m) => m.primary)
    ?? monitors[0];
  // Nudge range follows the real screen so the notch can travel edge to
  // edge. `window.screen` is always available (even if the monitor list
  // hasn't loaded); the detected monitor wins when present.
  const screenW = activeMonitor?.width ?? window.screen.availWidth;
  const screenH = activeMonitor?.height ?? window.screen.availHeight;
  const boundX = Math.max(100, Math.round(screenW / 2 / 10) * 10);
  const boundY = Math.max(100, Math.round(screenH / 2 / 10) * 10);
  // The settings panel never wears the custom surface: solid chrome that
  // follows the mode (dark / light / OS) instead.
  const chrome = settings.mode === "light" || (settings.mode === "system" && sysLight)
    ? "#f2f3f5"
    : "#090909";
  const lastFit = useRef(0);

  useEffect(() => { void getSnapshots().then(setSnapshots).catch(() => undefined); }, []);
  useEffect(() => { void listMonitors().then(setMonitors).catch(() => undefined); }, []);
  useEffect(() => {
    let live = true;
    void autohideSupported().then((supported) => {
      if (live) setAutoHideAvailable(supported);
    });
    return () => { live = false; };
  }, []);

  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(true);
  useEffect(() => {
    let live = true;
    setCheckingUpdate(true);
    checkForUpdates()
      .then((info) => { if (live) { setUpdateInfo(info); setCheckingUpdate(false); } })
      .catch(() => { if (live) setCheckingUpdate(false); });
    return () => { live = false; };
  }, []);
  const recheckUpdates = () => {
    setCheckingUpdate(true);
    checkForUpdates(true)
      .then((info) => { setUpdateInfo(info); setCheckingUpdate(false); })
      .catch(() => setCheckingUpdate(false));
  };

  // Measure content and size the window to fit it: no scrolling, no clipping.
  // Reports on mount, on viewport changes, whenever data arrives, AND on a
  // 1s poll — ResizeObserver alone can't see content growth inside a fixed
  // element, and late layout (fonts, images) can shift heights after mount.
  useLayoutEffect(() => {
    const el = document.querySelector(".settings-page");
    if (!(el instanceof HTMLElement)) return;
    const report = () => {
      const height = Math.ceil(el.scrollHeight);
      if (height > 0 && Math.abs(height - lastFit.current) > 2) {
        lastFit.current = height;
        void fitSettings(height);
      }
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    const poll = window.setInterval(report, 1000);
    return () => {
      observer.disconnect();
      window.clearInterval(poll);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshots.length, monitors.length, settings.autoHide]);

  const update = (next: ClientSettings) => {
    setSettings(next);
    saveSettings(next);
    if (runningInTauri()) void emitTo("notch", "settings:changed");
  };

  const autoHideDisabled = autoHideAvailable !== true;
  const delayIsPreset = autoHideDelays.includes(settings.autoHideDelaySec);

  return (
    <main className="settings-page" style={themeVars(chrome)}>
      <motion.header
        className="settings-header"
        {...rise}
        transition={{ duration: 0.25, ease: "easeOut" }}
      >
        <div className="drag-region" data-tauri-drag-region style={{ flex: 1 }}>
          <p className="eyebrow">CODENOTCH</p>
          <h1>Settings</h1>
        </div>
        <button type="button" className="icon-button" aria-label="Close" onClick={() => void hideWindow("settings")}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
            <path d="M2 2l8 8M10 2l-8 8" />
          </svg>
        </button>
      </motion.header>

      <motion.section
        className="settings-section"
        {...rise}
        transition={{ duration: 0.25, delay: 0.05, ease: "easeOut" }}
      >
        <h2>Screen edge</h2>
        <div className="edge-picker">
          {edges.map((edge) => (
            <motion.button
              key={edge}
              type="button"
              className={settings.edge === edge ? "is-selected" : ""}
              onClick={() => update({ ...settings, edge, offsetX: 0, offsetY: 0 })}
              whileTap={{ scale: 0.95 }}
              transition={{ type: "spring", stiffness: 500, damping: 25 }}
            >{edge}</motion.button>
          ))}
        </div>
      </motion.section>

      <motion.section
        className="settings-section"
        {...rise}
        transition={{ duration: 0.25, delay: 0.1, ease: "easeOut" }}
      >
        <h2>Providers</h2>
        <div className="provider-settings">
          {snapshots.map((snapshot, index) => {
            const active = settings.enabledProviders.includes(snapshot.id);
            const account = [snapshot.account?.label, snapshot.account?.plan].filter(Boolean).join(" · ");
            return (
              <motion.label
                className="provider-setting"
                key={snapshot.id}
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 0.22, delay: 0.12 + index * 0.04, ease: "easeOut" }}
              >
                <span className="settings-provider-glyph"><ProviderLogo id={snapshot.id} glyph={snapshot.glyph} size={16} /></span>
                <span className="provider-setting-copy">
                  <strong>{snapshot.displayName}</strong>
                  <small>{account || snapshot.account?.source || snapshot.message || "Local account discovery"}</small>
                </span>
                <input
                  type="checkbox"
                  checked={active}
                  onChange={(event) => update({
                    ...settings,
                    enabledProviders: event.target.checked
                      ? Array.from(new Set([...settings.enabledProviders, snapshot.id]))
                      : settings.enabledProviders.filter((id) => id !== snapshot.id),
                  })}
                />
              </motion.label>
            );
          })}
        </div>
      </motion.section>

      <motion.section
        className="settings-section"
        {...rise}
        transition={{ duration: 0.25, delay: 0.12, ease: "easeOut" }}
      >
        <h2>Position</h2>
        <div className="appearance-duo">
          <div>
            <p className="appearance-label">Nudge X <output>{Math.round(settings.offsetX)}px</output></p>
            <input
              type="range"
              className="opacity-slider"
              min={-boundX}
              max={boundX}
              step={5}
              value={Math.round(settings.offsetX)}
              aria-label="Horizontal offset"
              onChange={(event) => update({ ...settings, offsetX: Number(event.target.value) })}
            />
          </div>
          <div>
            <p className="appearance-label">Nudge Y <output>{Math.round(settings.offsetY)}px</output></p>
            <input
              type="range"
              className="opacity-slider"
              min={-boundY}
              max={boundY}
              step={5}
              value={Math.round(settings.offsetY)}
              aria-label="Vertical offset"
              onChange={(event) => update({ ...settings, offsetY: Number(event.target.value) })}
            />
          </div>
        </div>
        <div className="monitor-row">
          <p className="appearance-label">Monitor</p>
          <select
            className="monitor-select"
            value={monitors.some((m) => m.id === settings.monitor) ? settings.monitor : "primary"}
            aria-label="Monitor"
            onChange={(event) => update({ ...settings, monitor: event.target.value })}
          >
            <option value="primary">Primary</option>
            {monitors.filter((m) => !m.primary).map((m) => (
              <option key={m.id} value={m.id}>
                {(m.name ?? `Display ${Number(m.id) + 1}`) + ` · ${Math.round(m.width)}×${Math.round(m.height)}`}
              </option>
            ))}
          </select>
        </div>
      </motion.section>

      <motion.section
        className="settings-section"
        {...rise}
        transition={{ duration: 0.25, delay: 0.15, ease: "easeOut" }}
      >
        <h2>Appearance</h2>
        <div className="appearance-duo">
          <div>
            <p className="appearance-label">Mode</p>
            <div className="mode-segmented">
              {modes.map((mode) => (
                <motion.button
                  key={mode}
                  type="button"
                  className={settings.mode === mode ? "is-selected" : ""}
                  onClick={() => update({
                    ...settings,
                    mode,
                    // Modes own their surface: picking one resets any custom
                    // color so dark/light/system always look intentional.
                    ...(mode === "system" ? {} : { surface: mode === "light" ? LIGHT_SURFACE : DARK_SURFACE }),
                  })}
                  whileTap={{ scale: 0.95 }}
                  transition={{ type: "spring", stiffness: 500, damping: 25 }}
                >{mode}</motion.button>
              ))}
            </div>
          </div>
          <div>
            <p className="appearance-label">Surface <output>{settings.surface.toUpperCase()}</output></p>
            <div className="color-row">
              <input
                type="color"
                className="surface-picker"
                value={settings.surface}
                aria-label="Surface color"
                onChange={(event) => {
                  const value = event.target.value;
                  update({
                    ...settings,
                    surface: value,
                    mode: surfaceLuminance(value) > 0.5 ? "light" : "dark",
                  });
                }}
              />
              <span className="surface-hex">{settings.surface.toUpperCase()}</span>
            </div>
          </div>
        </div>
        <div className="appearance-duo">
          <div>
            <p className="appearance-label">Opacity <output>{Math.round(settings.opacity * 100)}%</output></p>
            <input
              type="range"
              className="opacity-slider"
              min={0}
              max={100}
              step={1}
              value={Math.round(settings.opacity * 100)}
              aria-label="Notch opacity"
              onChange={(event) => update({ ...settings, opacity: Number(event.target.value) / 100 })}
            />
          </div>
          <div>
            <p className="appearance-label">Notch size <output>{Math.round(settings.scale * 100)}%</output></p>
            <input
              type="range"
              className="opacity-slider"
              min={70}
              max={130}
              step={5}
              value={Math.round(settings.scale * 100)}
              aria-label="Notch size"
              onChange={(event) => update({ ...settings, scale: Number(event.target.value) / 100 })}
            />
          </div>
        </div>
      </motion.section>

      <motion.section
        className="settings-section"
        {...rise}
        transition={{ duration: 0.25, delay: 0.18, ease: "easeOut" }}
      >
        <h2>Auto-hide</h2>
        <div className="provider-settings">
          <label className="provider-setting">
            <span className="provider-setting-copy">
              <strong>Auto-hide when idle</strong>
              <small>Retract to a small edge sliver until you hover there again.</small>
            </span>
            <input
              type="checkbox"
              checked={settings.autoHide}
              disabled={autoHideDisabled}
              onChange={(event) => update({ ...settings, autoHide: event.target.checked })}
            />
          </label>
        </div>
        {settings.autoHide && (
          <div className="monitor-row">
            <p className="appearance-label">Hide after</p>
            <select
              className="monitor-select"
              value={settings.autoHideDelaySec}
              aria-label="Auto-hide delay"
              disabled={autoHideDisabled}
              onChange={(event) => update({ ...settings, autoHideDelaySec: Number(event.target.value) })}
            >
              {!delayIsPreset && (
                <option value={settings.autoHideDelaySec}>{settings.autoHideDelaySec} seconds</option>
              )}
              {autoHideDelays.map((seconds) => (
                <option key={seconds} value={seconds}>{seconds} seconds</option>
              ))}
            </select>
          </div>
        )}
        {autoHideAvailable === false && (
          <p className="appearance-label">Needs cursor position access — unavailable on this system.</p>
        )}
      </motion.section>

      <motion.section
        className="settings-section"
        {...rise}
        transition={{ duration: 0.25, delay: 0.2, ease: "easeOut" }}
      >
        <h2>Updates</h2>
        <div className={`update-row${updateInfo?.available ? " is-available" : ""}`}>
          <span className={`update-dot${updateInfo?.available ? " is-available" : checkingUpdate ? " is-checking" : ""}`} aria-hidden="true" />
          <span className="update-copy">
            {checkingUpdate
              ? "Checking for updates…"
              : updateInfo?.available
                ? `Version ${updateInfo.latest} is out — you're on ${updateInfo.current}`
                : updateInfo
                  ? `You're up to date (${updateInfo.current})`
                  : "Couldn't reach GitHub just now"}
          </span>
          {updateInfo?.available
            ? (
              <button type="button" className="download-button" onClick={() => void openExternal(updateInfo.url)}>
                Download
              </button>
            )
            : (
              <button type="button" className="reset-link" onClick={recheckUpdates} disabled={checkingUpdate}>
                Check again
              </button>
            )}
        </div>
      </motion.section>

      <motion.section
        className="settings-section source-note"
        {...rise}
        transition={{ duration: 0.25, delay: 0.22, ease: "easeOut" }}
      >
        <h2>How readings work</h2>
        <p>Codex reads local rollout logs. Cursor uses its local state database. Claude uses Claude Code&apos;s OAuth credential. OpenCode polls Zen usage with your saved API key. Antigravity reads its OS-keyring login. This app never writes provider credentials.</p>
      </motion.section>
      <footer className="settings-footer">
        <span>Windows + Linux clean-room port · v0.2.0</span>
        <span className="footer-links">
          <button type="button" className="reset-link" onClick={() => void openExternal(REPO_URL)}>
            GitHub
          </button>
          <button type="button" className="reset-link" onClick={() => update({ ...DEFAULT_SETTINGS })}>
            Reset to defaults
          </button>
        </span>
      </footer>
    </main>
  );
}
