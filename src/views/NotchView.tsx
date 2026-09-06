import { emitTo, listen } from "@tauri-apps/api/event";
import { motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  cursorOverTooltipArea,
  getSnapshots,
  hideTooltip,
  openProvider,
  openSettings,
  runningInTauri,
  setBlur,
  setEdge,
  showContextMenu,
  showTooltip,
} from "../lib/backend";
import { loadSettings } from "../lib/settings";
import { bandFor, resolveSurface, themeVars, useSystemLight } from "../lib/theme";
import { clamp01, formatPercent, headlineWindow } from "../lib/usage";
import { ProviderLogo } from "../components/ProviderLogo";
import type { Edge, ProviderSnapshot } from "../types";

const ringSize = 44;
const trackStroke = 5.8;
const progressStroke = 3;
const radius = (ringSize - trackStroke) / 2;
const circumference = 2 * Math.PI * radius;

function ProviderRing({ snapshot, surface }: { snapshot: ProviderSnapshot; surface: string }) {
  const window = headlineWindow(snapshot);
  const used = window?.usedFraction;
  const fraction = clamp01(used ?? 0);
  const isWaiting = snapshot.activity?.state === "waiting";
  const color = bandFor(surface, isWaiting ? 1 : fraction);
  const stale = snapshot.status === "stale" || snapshot.status === "error";

  return (
    <div className={`provider-ring ${stale ? "is-stale" : ""}`}>
      <svg className="ring-svg" viewBox="0 0 44 44" aria-hidden="true">
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
      <span className="provider-glyph" aria-hidden="true"><ProviderLogo id={snapshot.id} glyph={snapshot.glyph} /></span>
    </div>
  );
}

function ProviderCell({
  snapshot,
  index,
  edge,
  surface,
  onHover,
  onLeave,
}: {
  snapshot: ProviderSnapshot;
  index: number;
  edge: Edge;
  surface: string;
  onHover: (snapshot: ProviderSnapshot, index: number) => void;
  onLeave: () => void;
}) {
  const used = headlineWindow(snapshot)?.usedFraction;
  const slide = edge === "right" || edge === "left"
    ? { x: 0, y: 12 }
    : { x: 12, y: 0 };
  return (
    <motion.button
      type="button"
      className="provider-cell"
      aria-label={`${snapshot.displayName} ${used == null ? "usage unavailable" : `${formatPercent(used)} used`}`}
      onMouseEnter={() => onHover(snapshot, index)}
      onMouseLeave={onLeave}
      onFocus={() => onHover(snapshot, index)}
      onBlur={onLeave}
      onClick={() => void openProvider(snapshot)}
      data-edge={edge}
      initial={{ opacity: 0, scale: 0.8, ...slide }}
      animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
      whileHover={{ scale: 1.07 }}
      whileTap={{ scale: 0.94 }}
      transition={{ type: "spring", stiffness: 420, damping: 26, delay: Math.min(index * 0.05, 0.25) }}
    >
      <ProviderRing snapshot={snapshot} surface={surface} />
      <span className="provider-percent">{used == null ? (snapshot.displayValue ?? "—") : formatPercent(used)}</span>
    </motion.button>
  );
}

export function NotchView() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [settings, setSettings] = useState(loadSettings);
  const sysLight = useSystemLight();
  const surface = resolveSurface(settings.mode, settings.surface, sysLight);
  const shellStyle = { ...themeVars(surface, settings.opacity), zoom: settings.scale };
  const leaveTimer = useRef<number | null>(null);
  // Invalidates in-flight hide timers (see `leave`).
  const leaveGen = useRef(0);
  // Currently visible tooltip card, if any. Used to skip redundant re-shows
  // and to give a freshly shown card a grace period against spurious
  // leave/enter pairs from window activation.
  const tipRef = useRef<{ id: string; at: number } | null>(null);
  const edge = settings.edge;

  const enabled = useMemo(
    () => snapshots.filter((item) => settings.enabledProviders.includes(item.id)),
    [settings, snapshots],
  );

  const refresh = useCallback(async () => {
    try {
      setSnapshots(await getSnapshots());
    } catch {
      // Keep the last good readings rather than blanking the notch.
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(id);
  }, [refresh]);

  useEffect(() => {
    // Any notch move invalidates tooltip placement: drop the card so the next
    // hover re-anchors it against the new geometry.
    leaveGen.current++;
    tipRef.current = null;
    void hideTooltip();
    void setEdge(
      edge,
      Math.max(enabled.length, 1),
      settings.scale,
      settings.monitor,
      settings.offsetX,
      settings.offsetY,
    );
    void setBlur(settings.blur);
  }, [edge, enabled.length, settings.scale, settings.monitor, settings.offsetX, settings.offsetY, settings.blur]);

  const doHide = useCallback(() => {
    tipRef.current = null;
    void hideTooltip();
  }, []);

  const leave = useCallback(() => {
    const gen = ++leaveGen.current;
    if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => {
      void (async () => {
        try {
          const inside = await cursorOverTooltipArea();
          // A newer leave/hover superseded this timer while verifying.
          if (gen !== leaveGen.current) return;
          if (inside) {
            leave();
            return;
          }
        } catch {
          // Unknown position: fall through to the plain timeout behavior.
        }
        if (gen !== leaveGen.current) return;
        doHide();
      })();
    }, 250);
  }, [doHide]);

  useEffect(() => {
    if (!runningInTauri()) return;
    const unlisten: Array<() => void> = [];
    void listen("app:refresh", () => void refresh()).then((fn) => unlisten.push(fn));
    void listen("settings:changed", () => setSettings(loadSettings())).then((fn) => unlisten.push(fn));
    void listen("tooltip:hover", () => {
      if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    }).then((fn) => unlisten.push(fn));
    void listen("tooltip:leave", () => {
      leave();
    }).then((fn) => unlisten.push(fn));
    return () => unlisten.forEach((fn) => fn());
  }, [refresh, leave]);

  const hover = (snapshot: ProviderSnapshot, index: number) => {
    leaveGen.current++;
    if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    // Same card already visible: skip the re-show (prevents show/hide flapping).
    if (tipRef.current?.id === snapshot.id) return;
    tipRef.current = { id: snapshot.id, at: Date.now() };
    if (runningInTauri()) {
      void emitTo("tooltip", "tooltip:show", { snapshot, edge, index });
    }
    void showTooltip(edge, index, settings.scale).catch(() => {
      tipRef.current = null;
    });
  };

  if (!enabled.length) {
    return (
      <main className={`notch-shell edge-${edge}`} style={shellStyle} onContextMenu={(event) => {
        event.preventDefault();
        void showContextMenu(edge, settings.scale);
      }}>
        <button className="settings-orb empty" type="button" onClick={() => void openSettings()} aria-label="Settings">
          <span className="orb-dot" aria-hidden="true" />
          <svg className="orb-cog" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </main>
    );
  }

  return (
    <main
      className={`notch-shell edge-${edge}`}
      style={shellStyle}
      onContextMenu={(event) => {
        event.preventDefault();
        void showContextMenu(edge, settings.scale);
      }}
    >
      <div className="provider-stack">
        {enabled.map((snapshot, index) => (
            <ProviderCell
              key={snapshot.id}
              snapshot={snapshot}
              index={index}
              edge={edge}
              surface={surface}
              onHover={hover}
              onLeave={leave}
            />
        ))}
      </div>
      <button className="settings-orb" type="button" onClick={() => void openSettings()} aria-label="Settings">
        <span className="orb-dot" aria-hidden="true" />
        <svg className="orb-cog" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </button>
    </main>
  );
}
