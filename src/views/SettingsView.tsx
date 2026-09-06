import { emitTo } from "@tauri-apps/api/event";
import { motion } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { fitSettings, getSnapshots, hideWindow, listMonitors, runningInTauri } from "../lib/backend";
import { loadSettings, saveSettings } from "../lib/settings";
import { resolveSurface, surfaceLuminance, themeVars, useSystemLight } from "../lib/theme";
import { ProviderLogo } from "../components/ProviderLogo";
import type { ClientSettings, Edge, MonitorInfo, ProviderSnapshot, ThemeMode } from "../types";

const edges: Edge[] = ["right", "left", "top", "bottom"];
const modes: ThemeMode[] = ["dark", "light", "system"];

const rise = {
  initial: { opacity: 0, y: 10 },
  animate: { opacity: 1, y: 0 },
};

export function SettingsView() {
  const [settings, setSettings] = useState<ClientSettings>(loadSettings);
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const sysLight = useSystemLight();
  const surface = resolveSurface(settings.mode, settings.surface, sysLight);
  const lastFit = useRef(0);

  useEffect(() => { void getSnapshots().then(setSnapshots).catch(() => undefined); }, []);
  useEffect(() => { void listMonitors().then(setMonitors).catch(() => undefined); }, []);

  // Measure content and size the window to fit it: no scrolling, no clipping.
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
    return () => observer.disconnect();
  }, []);

  const update = (next: ClientSettings) => {
    setSettings(next);
    saveSettings(next);
    if (runningInTauri()) void emitTo("notch", "settings:changed");
  };

  return (
    <main className="settings-page" style={themeVars(surface, 1)}>
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
              onClick={() => update({ ...settings, edge })}
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
              min={-200}
              max={200}
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
              min={-200}
              max={200}
              step={5}
              value={Math.round(settings.offsetY)}
              aria-label="Vertical offset"
              onChange={(event) => update({ ...settings, offsetY: Number(event.target.value) })}
            />
          </div>
        </div>
        <div className="appearance-duo">
          <div>
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
          <div>
            <p className="appearance-label">Background blur</p>
            <label className="mini-toggle">
              <input
                type="checkbox"
                checked={settings.blur}
                onChange={(event) => update({ ...settings, blur: event.target.checked })}
              />
              <span>{settings.blur ? "Frosted glass on" : "Solid background"}</span>
            </label>
          </div>
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
                  onClick={() => update({ ...settings, mode })}
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
                min={40}
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
        className="settings-section source-note"
        {...rise}
        transition={{ duration: 0.25, delay: 0.2, ease: "easeOut" }}
      >
        <h2>How readings work</h2>
        <p>Codex reads local rollout logs. Cursor uses its local state database. Claude uses Claude Code&apos;s OAuth credential. OpenCode polls Zen usage with your saved API key. Antigravity reads its OS-keyring login. This app never writes provider credentials.</p>
      </motion.section>
      <footer className="settings-footer">Windows + Linux clean-room port · v0.1.0</footer>
    </main>
  );
}
