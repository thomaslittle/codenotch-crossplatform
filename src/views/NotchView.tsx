import { emitTo, listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getSnapshots,
  hideTooltip,
  openProvider,
  openSettings,
  runningInTauri,
  setEdge,
  showContextMenu,
  showTooltip,
} from "../lib/backend";
import { loadSettings } from "../lib/settings";
import { bandColor, clamp01, formatPercent, headlineWindow } from "../lib/usage";
import type { Edge, ProviderSnapshot } from "../types";

const ringSize = 44;
const trackStroke = 5.8;
const progressStroke = 3;
const radius = (ringSize - trackStroke) / 2;
const circumference = 2 * Math.PI * radius;

function ProviderRing({ snapshot }: { snapshot: ProviderSnapshot }) {
  const window = headlineWindow(snapshot);
  const used = window?.usedFraction;
  const fraction = clamp01(used ?? 0);
  const isWaiting = snapshot.activity?.state === "waiting";
  const color = bandColor(isWaiting ? 1 : fraction);
  const stale = snapshot.status === "stale" || snapshot.status === "error";

  return (
    <div className={`provider-ring ${stale ? "is-stale" : ""}`}>
      <svg viewBox="0 0 44 44" aria-hidden="true">
        <circle className="ring-track" cx="22" cy="22" r={radius} strokeWidth={trackStroke} />
        {used != null && (
          <circle
            className="ring-value"
            cx="22"
            cy="22"
            r={radius}
            stroke={color}
            strokeWidth={progressStroke}
            strokeDasharray={circumference}
            strokeDashoffset={circumference * (1 - fraction)}
          />
        )}
        {snapshot.activity?.state === "working" && (
          <circle className="activity-spinner" cx="22" cy="22" r="13.4" />
        )}
        {snapshot.activity?.state === "waiting" && (
          <circle className="activity-waiting" cx="22" cy="22" r="13.4" />
        )}
      </svg>
      <span className="provider-glyph" aria-hidden="true">{snapshot.glyph}</span>
    </div>
  );
}

function ProviderCell({
  snapshot,
  index,
  edge,
  onHover,
  onLeave,
}: {
  snapshot: ProviderSnapshot;
  index: number;
  edge: Edge;
  onHover: (snapshot: ProviderSnapshot, index: number) => void;
  onLeave: () => void;
}) {
  const used = headlineWindow(snapshot)?.usedFraction;
  return (
    <button
      type="button"
      className="provider-cell"
      aria-label={`${snapshot.displayName} ${used == null ? "usage unavailable" : `${formatPercent(used)} used`}`}
      onMouseEnter={() => onHover(snapshot, index)}
      onMouseLeave={onLeave}
      onFocus={() => onHover(snapshot, index)}
      onBlur={onLeave}
      onClick={() => void openProvider(snapshot)}
      data-edge={edge}
    >
      <ProviderRing snapshot={snapshot} />
      <span className="provider-percent">{used == null ? (snapshot.displayValue ?? "—") : formatPercent(used)}</span>
    </button>
  );
}

export function NotchView() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [settings, setSettings] = useState(loadSettings);
  const leaveTimer = useRef<number | null>(null);
  const edge = settings.edge;

  const enabled = useMemo(
    () => snapshots.filter((item) => settings.enabledProviders.includes(item.id)),
    [settings, snapshots],
  );

  const refresh = useCallback(async () => {
    const next = await getSnapshots();
    setSnapshots(next);
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(id);
  }, [refresh]);

  useEffect(() => {
    void setEdge(edge, Math.max(enabled.length, 1));
  }, [edge, enabled.length]);

  useEffect(() => {
    if (!runningInTauri()) return;
    const unlisten: Array<() => void> = [];
    void listen("app:refresh", () => void refresh()).then((fn) => unlisten.push(fn));
    void listen("settings:changed", () => setSettings(loadSettings())).then((fn) => unlisten.push(fn));
    void listen("tooltip:hover", () => {
      if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    }).then((fn) => unlisten.push(fn));
    void listen("tooltip:leave", () => {
      leaveTimer.current = window.setTimeout(() => void hideTooltip(), 250);
    }).then((fn) => unlisten.push(fn));
    return () => unlisten.forEach((fn) => fn());
  }, [refresh]);

  const hover = (snapshot: ProviderSnapshot, index: number) => {
    if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    if (runningInTauri()) {
      void emitTo("tooltip", "tooltip:show", { snapshot, edge, index });
    }
    void showTooltip(edge, index);
  };

  const leave = () => {
    leaveTimer.current = window.setTimeout(() => void hideTooltip(), 250);
  };

  if (!enabled.length) {
    return (
      <main className={`notch-shell edge-${edge}`} onContextMenu={(event) => {
        event.preventDefault();
        void showContextMenu(edge);
      }}>
        <button className="settings-orb empty" type="button" onClick={() => void openSettings()} aria-label="Settings"><span aria-hidden="true">⚙</span></button>
      </main>
    );
  }

  return (
    <main
      className={`notch-shell edge-${edge}`}
      onContextMenu={(event) => {
        event.preventDefault();
        void showContextMenu(edge);
      }}
    >
      <div className="provider-stack">
        {enabled.map((snapshot, index) => (
          <ProviderCell
            key={snapshot.id}
            snapshot={snapshot}
            index={index}
            edge={edge}
            onHover={hover}
            onLeave={leave}
          />
        ))}
      </div>
      <button className="settings-orb" type="button" onClick={() => void openSettings()} aria-label="Settings"><span aria-hidden="true">⚙</span></button>
    </main>
  );
}
