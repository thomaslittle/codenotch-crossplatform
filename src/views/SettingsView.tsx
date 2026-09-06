import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { autohideSupported, REPO_URL, fitSettings, getSnapshots, hideWindow, listMonitors, openExternal, runningInTauri } from "../lib/backend";
import { DEFAULT_SETTINGS, loadSettings, saveSettings } from "../lib/settings";
import { DARK_SURFACE, LIGHT_SURFACE, THEME_PRESETS, surfaceLuminance, themeVars, useSystemLight } from "../lib/theme";
import { checkForUpdates, type UpdateInfo } from "../lib/updates";
import { ProviderLogo } from "../components/ProviderLogo";
import type { ClientSettings, Edge, GaugeStyle, MonitorInfo, ProviderSnapshot, ShellStyle, ThemeMode } from "../types";

const edges: Edge[] = ["right", "left", "top", "bottom"];
const modes: ThemeMode[] = ["dark", "light", "system"];
const shellStyles: Array<{ id: ShellStyle; label: string; description: string }> = [
  { id: "tab", label: "Tab", description: "The original moulded screen-edge tab." },
  { id: "bubble", label: "Bubble", description: "Separate rounded provider bubbles with no shared body." },
  { id: "sharp", label: "Sharp", description: "Crisp rectangular body with minimal rounding." },
  { id: "trapezoid", label: "Trapezoid", description: "Angled inner edge for a more technical silhouette." },
  { id: "pill", label: "Pill", description: "Soft capsule body with strong rounded corners." },
  { id: "rail", label: "Rail", description: "Thin dock rail behind the gauges for a compact look." },
  { id: "dock", label: "Dock", description: "Floating translucent desktop dock with rounded glass chrome." },
  { id: "ghost", label: "Ghost", description: "No shared background — gauges float directly over the desktop." },
];
const gaugeStyles: Array<{ id: GaugeStyle; label: string; description: string }> = [
  { id: "classic", label: "Classic", description: "Original circular headline usage ring with percentage." },
  { id: "slim", label: "Slim", description: "Provider icon with one clean horizontal headline meter." },
  { id: "halo", label: "Halo", description: "Segmented circular headline gauge with active usage ticks." },
  { id: "stacked", label: "Stacked", description: "Compact labeled C/W/M-style bars showing up to three real quota windows." },
  { id: "columns", label: "Columns", description: "Compact vertical meters showing up to three real quota windows." },
  { id: "micro", label: "Micro", description: "Ultra-compact provider icon plus up to three horizontal usage rails." },
];
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
  const screenW = activeMonitor?.width ?? window.screen.availWidth;
  const screenH = activeMonitor?.height ?? window.screen.availHeight;
  const boundX = Math.max(100, Math.round(screenW / 2 / 10) * 10);
  const boundY = Math.max(100, Math.round(screenH / 2 / 10) * 10);
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

  useLayoutEffect(() => {
    const el = document.querySelector(".settings-page");
    if (!(el instanceof HTMLElement)) return;
    const report = () => {
      const height = Math.ceil(el.scrollHeight);
      if (height > 0 && Math.abs(height - lastFit.current) > 2) {
        lastFit.current = height;
        void fitSettings(height).then(() => {
          if (!runningInTauri()) return;
          const settingsWindow = getCurrentWindow();
          void settingsWindow.center().catch(() => undefined);
          window.setTimeout(() => {
            void settingsWindow.center().catch(() => undefined);
          }, 50);
        });
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

  const closeSettings = () => {
    if (runningInTauri()) void emitTo("notch", "settings:closed");
    void hideWindow("settings");
  };

  const autoHideDisabled = autoHideAvailable !== true;
  const delayIsPreset = autoHideDelays.includes(settings.autoHideDelaySec);

  return (
    <main className="settings-page" style={themeVars(chrome)}>
      <motion.header
        className="settings-header"
        data-tauri-drag-region
        {...rise}
        transition={{ duration: 0.25, ease: "easeOut" }}
      >
        <div className="drag-region" data-tauri-drag-region style={{ flex: 1 }}>
          <p className="eyebrow" data-tauri-drag-region>CODENOTCH</p>
          <h1 data-tauri-drag-region>Settings</h1>
        </div>
        <button type="button" className="icon-button" aria-label="Close" onClick={closeSettings}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
            <path d="M2 2l8 8M10 2l-8 8" />
          </svg>
        </button>
      </motion.header>

      <motion.section className="settings-section" {...rise} transition={{ duration: 0.25, delay: 0.05, ease: "easeOut" }}>
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

      <motion.section className="settings-section" {...rise} transition={{ duration: 0.25, delay: 0.1, ease: "easeOut" }}>
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

      <motion.section className="settings-section" {...rise} transition={{ duration: 0.25, delay: 0.12, ease: "easeOut" }}>
        <h2>Position</h2>
        <div className="appearance-duo">
          <div>
            <p className="appearance-label">Nudge X <output>{Math.round(settings.offsetX)}px</output></p>
            <input type="range" className="opacity-slider" min={-boundX} max={boundX} step={5} value={Math.round(settings.offsetX)} aria-label="Horizontal offset" onChange={(event) => update({ ...settings, offsetX: Number(event.target.value) })} />
          </div>
          <div>
            <p className="appearance-label">Nudge Y <output>{Math.round(settings.offsetY)}px</output></p>
            <input type="range" className="opacity-slider" min={-boundY} max={boundY} step={5} value={Math.round(settings.offsetY)} aria-label="Vertical offset" onChange={(event) => update({ ...settings, offsetY: Number(event.target.value) })} />
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

      <motion.section className="settings-section" {...rise} transition={{ duration: 0.25, delay: 0.15, ease: "easeOut" }}>
        <h2>Appearance</h2>
        <p className="appearance-label">Theme</p>
        <div className="mode-segmented">
          {THEME_PRESETS.map((preset) => (
            <motion.button
              key={preset.id}
              type="button"
              className={settings.theme === preset.id ? "is-selected" : ""}
              title={preset.description}
              onClick={() => update(preset.id === "custom"
                ? { ...settings, theme: "custom" }
                : { ...settings, theme: preset.id, mode: "dark", surface: preset.surface })}
              whileTap={{ scale: 0.95 }}
              transition={{ type: "spring", stiffness: 500, damping: 25 }}
            >
              <span
                aria-hidden="true"
                style={{
                  display: "inline-block",
                  width: 8,
                  height: 8,
                  marginRight: 6,
                  borderRadius: "50%",
                  background: preset.id === "custom" ? "linear-gradient(135deg, #fff 0 50%, #000 50%)" : preset.surface,
                  border: "1px solid currentColor",
                  verticalAlign: "-1px",
                }}
              />
              {preset.label}
            </motion.button>
          ))}
        </div>

        <p className="appearance-label" style={{ marginTop: 12 }}>Shell</p>
        <div className="mode-segmented shell-picker">
          {shellStyles.map((shell) => (
            <motion.button
              key={shell.id}
              type="button"
              className={settings.shellStyle === shell.id ? "is-selected" : ""}
              title={shell.description}
              onClick={() => update({ ...settings, shellStyle: shell.id })}
              whileTap={{ scale: 0.95 }}
              transition={{ type: "spring", stiffness: 500, damping: 25 }}
            >{shell.label}</motion.button>
          ))}
        </div>

        <p className="appearance-label" style={{ marginTop: 12 }}>Gauge</p>
        <div className="mode-segmented">
          {gaugeStyles.map((gauge) => (
            <motion.button
              key={gauge.id}
              type="button"
              className={settings.gaugeStyle === gauge.id ? "is-selected" : ""}
              title={gauge.description}
              onClick={() => update({ ...settings, gaugeStyle: gauge.id })}
              whileTap={{ scale: 0.95 }}
              transition={{ type: "spring", stiffness: 500, damping: 25 }}
            >{gauge.label}</motion.button>
          ))}
        </div>
        <div className="appearance-duo">
          <div>
            <p className="appearance-label">Mode</p>
            <div className="mode-segmented">
              {modes.map((mode) => (
                <motion.button
                  key={mode}
                  type="button"
                  className={settings.mode === mode && settings.theme === "custom" ? "is-selected" : ""}
                  onClick={() => update({
                    ...settings,
                    theme: "custom",
                    mode,
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
                    theme: "custom",
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
            <input type="range" className="opacity-slider" min={0} max={100} step={1} value={Math.round(settings.opacity * 100)} aria-label="Notch opacity" onChange={(event) => update({ ...settings, opacity: Number(event.target.value) / 100 })} />
          </div>
          <div>
            <p className="appearance-label">Notch size <output>{Math.round(settings.scale * 100)}%</output></p>
            <input type="range" className="opacity-slider" min={70} max={130} step={5} value={Math.round(settings.scale * 100)} aria-label="Notch size" onChange={(event) => update({ ...settings, scale: Number(event.target.value) / 100 })} />
          </div>
        </div>
      </motion.section>

      <motion.section className="settings-section" {...rise} transition={{ duration: 0.25, delay: 0.18, ease: "easeOut" }}>
        <h2>Auto-hide</h2>
        <div className="provider-settings">
          <label className="provider-setting">
            <span className="provider-setting-copy">
              <strong>Auto-hide when idle</strong>
              <small>Retract to a small edge sliver until you hover there again.</small>
            </span>
            <input type="checkbox" checked={settings.autoHide} disabled={autoHideDisabled} onChange={(event) => update({ ...settings, autoHide: event.target.checked })} />
          </label>
        </div>
        {settings.autoHide && (
          <div className="monitor-row">
            <p className="appearance-label">Hide after</p>
            <select className="monitor-select" value={settings.autoHideDelaySec} aria-label="Auto-hide delay" disabled={autoHideDisabled} onChange={(event) => update({ ...settings, autoHideDelaySec: Number(event.target.value) })}>
              {!delayIsPreset && <option value={settings.autoHideDelaySec}>{settings.autoHideDelaySec} seconds</option>}
              {autoHideDelays.map((seconds) => <option key={seconds} value={seconds}>{seconds} seconds</option>)}
            </select>
          </div>
        )}
        {autoHideAvailable === false && <p className="appearance-label">Needs cursor position access — unavailable on this system.</p>}
      </motion.section>

      <motion.section className="settings-section" {...rise} transition={{ duration: 0.25, delay: 0.2, ease: "easeOut" }}>
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
            ? <button type="button" className="download-button" onClick={() => void openExternal(updateInfo.url)}>Download</button>
            : <button type="button" className="reset-link" onClick={recheckUpdates} disabled={checkingUpdate}>Check again</button>}
        </div>
      </motion.section>

      <motion.section className="settings-section source-note" {...rise} transition={{ duration: 0.25, delay: 0.22, ease: "easeOut" }}>
        <h2>How readings work</h2>
        <p>Codex asks its local app server for live account limits and falls back to rollout logs. Cursor uses its local state database. Claude uses Claude Code&apos;s OAuth credential. OpenCode polls Go usage with your saved API key. Antigravity reads its OS-keyring login. This app never writes provider credentials.</p>
      </motion.section>
      <footer className="settings-footer">
        <span>Windows + Linux clean-room port · v0.3.1</span>
        <span className="footer-links">
          <button type="button" className="reset-link" onClick={() => void openExternal(REPO_URL)}>GitHub</button>
          <button type="button" className="reset-link" onClick={() => update({ ...DEFAULT_SETTINGS })}>Reset to defaults</button>
        </span>
      </footer>
    </main>
  );
}
