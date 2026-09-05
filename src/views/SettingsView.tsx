import { emitTo } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { getSnapshots, hideWindow, runningInTauri } from "../lib/backend";
import { loadSettings, saveSettings } from "../lib/settings";
import type { ClientSettings } from "../types";
import type { Edge, ProviderSnapshot } from "../types";

const edges: Edge[] = ["right", "left", "top", "bottom"];

export function SettingsView() {
  const [settings, setSettings] = useState<ClientSettings>(loadSettings);
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);

  useEffect(() => { void getSnapshots().then(setSnapshots); }, []);

  const update = (next: ClientSettings) => {
    setSettings(next);
    saveSettings(next);
    if (runningInTauri()) void emitTo("notch", "settings:changed");
  };

  return (
    <main className="settings-page">
      <header className="settings-header">
        <div>
          <p className="eyebrow">CODENOTCH</p>
          <h1>Settings</h1>
        </div>
        <button type="button" className="icon-button" aria-label="Close" onClick={() => void hideWindow("settings")}>×</button>
      </header>

      <section className="settings-section">
        <h2>Screen edge</h2>
        <div className="edge-picker">
          {edges.map((edge) => (
            <button
              key={edge}
              type="button"
              className={settings.edge === edge ? "is-selected" : ""}
              onClick={() => update({ ...settings, edge })}
            >{edge}</button>
          ))}
        </div>
      </section>

      <section className="settings-section">
        <h2>Providers</h2>
        <div className="provider-settings">
          {snapshots.map((snapshot) => {
            const active = settings.enabledProviders.includes(snapshot.id);
            const account = [snapshot.account?.label, snapshot.account?.plan].filter(Boolean).join(" · ");
            return (
              <label className="provider-setting" key={snapshot.id}>
                <span className="settings-provider-glyph">{snapshot.glyph}</span>
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
              </label>
            );
          })}
        </div>
      </section>

      <section className="settings-section source-note">
        <h2>How readings work</h2>
        <p>Codex is read from local rollout logs. Cursor reads the editor&apos;s own local state database and asks Cursor for its usage summary. Claude uses Claude Code&apos;s existing local OAuth credential when available. Antigravity reads its existing OS-keyring login and uses a local request-count fallback only when Google publishes no metered quota. This app never writes provider credentials.</p>
      </section>
      <footer className="settings-footer">Windows + Linux clean-room port · v0.1.0</footer>
    </main>
  );
}
