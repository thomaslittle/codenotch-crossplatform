import { emitTo, listen } from "@tauri-apps/api/event";
import { motion } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ComponentProps, CSSProperties, MouseEvent } from "react";
import {
  autohideSupported,
  cursorOverOverlay,
  cursorOverTooltipArea,
  getSnapshots,
  hideTooltip,
  openProvider,
  openSettings,
  runningInTauri,
  setEdge,
  setNotchRetracted,
  showContextMenu,
  showTooltip,
  trace,
} from "../lib/backend";
import { loadSettings } from "../lib/settings";
import { checkForUpdates } from "../lib/updates";
import { bandForRemaining, resolveSurface, themeVars, useSystemLight } from "../lib/theme";
import { clamp01, formatPercent, headlineWindow, remainingFraction } from "../lib/usage";
import {
  clearUsageResetAlerts,
  getUsageResetAlerts,
  processUsageResetSnapshots,
  type UsageResetAlert,
} from "../lib/usageAlerts";
import { ProviderLogo } from "../components/ProviderLogo";
import type { Edge, GaugeStyle, LimitWindow, ProviderSnapshot, ShellStyle } from "../types";

const ringSize = 44;
const classicTrackStroke = 5.8;
const classicProgressStroke = 3;
const classicRadius = (ringSize - classicTrackStroke) / 2;
const classicCircumference = 2 * Math.PI * classicRadius;
const resetGreen = "#00ff88";
const resetPeekSize = 36;
const resetPeekTravel = resetPeekSize - 6;

function isCompactGauge(style: GaugeStyle): boolean {
  return style === "stacked" || style === "columns" || style === "micro";
}

function isDockShell(style: ShellStyle): boolean {
  return style === "dock" || style === "dock3d";
}

function shellDepth(style: ShellStyle, edge: Edge, compact: boolean): number {
  const side = edge === "right" || edge === "left";
  if (style === "dock3d") {
    return side ? (compact ? 80 : 84) : (compact ? 90 : 96);
  }
  if (style === "dock" || style === "rail") {
    return side ? (compact ? 70 : 74) : (compact ? 80 : 86);
  }
  return side ? 70 : 84;
}

function compactLabel(window: LimitWindow): string {
  const id = window.id.toLowerCase();
  const label = window.label.toLowerCase();
  if (id.includes("week") || label.includes("week")) return "W";
  if (id.includes("month") || label.includes("month")) return "M";
  if (id.includes("day") || label.includes("day")) return "D";
  if (id.includes("rolling") || label.includes("current")) return "C";
  if (label.includes("5h") || label.includes("5 h") || label.includes("5 hour")) return "5";
  return window.label.trim().slice(0, 1).toUpperCase() || "•";
}

function visibleWindows(snapshot: ProviderSnapshot): LimitWindow[] {
  return snapshot.windows.slice(0, 3);
}

function resetSliverStyle(edge: Edge): CSSProperties {
  const base: CSSProperties = {
    position: "absolute",
    zIndex: 7,
    pointerEvents: "none",
    background: resetGreen,
    boxShadow: "0 0 10px rgba(0, 255, 136, .95)",
  };
  switch (edge) {
    case "right":
      return { ...base, left: 0, top: "calc(50% - 12px)", width: 6, height: 24, borderRadius: "0 6px 6px 0" };
    case "left":
      return { ...base, right: 0, top: "calc(50% - 12px)", width: 6, height: 24, borderRadius: "6px 0 0 6px" };
    case "top":
      return { ...base, bottom: 0, left: "calc(50% - 12px)", width: 24, height: 6, borderRadius: "6px 6px 0 0" };
    case "bottom":
      return { ...base, top: 0, left: "calc(50% - 12px)", width: 24, height: 6, borderRadius: "0 0 6px 6px" };
  }
}

function resetPeekStyle(edge: Edge): CSSProperties {
  const base: CSSProperties = {
    position: "absolute",
    zIndex: 8,
    width: resetPeekSize,
    height: resetPeekSize,
    borderRadius: 12,
    display: "grid",
    placeItems: "center",
    color: "var(--text)",
    background: "color-mix(in srgb, var(--black) 88%, transparent)",
    border: `1px solid ${resetGreen}`,
    boxShadow: "0 7px 18px rgba(0,0,0,.38), 0 0 14px rgba(0,255,136,.7)",
    backdropFilter: "blur(7px)",
    pointerEvents: "none",
  };
  switch (edge) {
    case "right": return { ...base, left: 0, top: `calc(50% - ${resetPeekSize / 2}px)` };
    case "left": return { ...base, right: 0, top: `calc(50% - ${resetPeekSize / 2}px)` };
    case "top": return { ...base, bottom: 0, left: `calc(50% - ${resetPeekSize / 2}px)` };
    case "bottom": return { ...base, top: 0, left: `calc(50% - ${resetPeekSize / 2}px)` };
  }
}

function resetPeekMotion(edge: Edge): { x: number[]; y: number[] } {
  switch (edge) {
    case "right": return { x: [0, -resetPeekTravel - 3, -resetPeekTravel, -resetPeekTravel, 0], y: [0, 0, 0, 0, 0] };
    case "left": return { x: [0, resetPeekTravel + 3, resetPeekTravel, resetPeekTravel, 0], y: [0, 0, 0, 0, 0] };
    case "top": return { x: [0, 0, 0, 0, 0], y: [0, resetPeekTravel + 3, resetPeekTravel, resetPeekTravel, 0] };
    case "bottom": return { x: [0, 0, 0, 0, 0], y: [0, -resetPeekTravel - 3, -resetPeekTravel, -resetPeekTravel, 0] };
  }
}

function ActivityOverlay({ snapshot }: { snapshot: ProviderSnapshot }) {
  if (!snapshot.activity || snapshot.activity.state === "idle") return null;
  return (
    <svg className="ring-svg" viewBox="0 0 44 44" aria-hidden="true">
      {snapshot.activity.state === "working" && (
        <circle className="activity-spinner" cx="22" cy="22" r="13.4" />
      )}
      {snapshot.activity.state === "waiting" && (
        <circle className="activity-waiting" cx="22" cy="22" r="13.4" />
      )}
    </svg>
  );
}

function GaugeBar({ window, surface, height = 3 }: { window: LimitWindow; surface: string; height?: number }) {
  const fraction = remainingFraction(window.usedFraction);
  const color = bandForRemaining(surface, fraction);
  return (
    <span
      title={`${window.label}: ${formatPercent(fraction)} remaining`}
      style={{
        display: "block",
        position: "relative",
        width: "100%",
        height,
        overflow: "hidden",
        borderRadius: 999,
        background: "var(--track)",
      }}
    >
      <span
        style={{
          display: "block",
          width: `${fraction * 100}%`,
          height: "100%",
          borderRadius: 999,
          background: color,
          transition: "width 420ms cubic-bezier(.22,.8,.2,1), background 220ms ease",
        }}
      />
    </span>
  );
}

function ProviderGauge({
  snapshot,
  surface,
  gaugeStyle,
}: {
  snapshot: ProviderSnapshot;
  surface: string;
  gaugeStyle: GaugeStyle;
}) {
  const headline = headlineWindow(snapshot);
  const used = headline?.usedFraction;
  const fraction = used == null ? 0 : remainingFraction(used);
  const color = bandForRemaining(surface, fraction);
  const stale = snapshot.status === "stale" || snapshot.status === "error";
  const windows = visibleWindows(snapshot);

  if (gaugeStyle === "slim") {
    return (
      <div className={`provider-ring gauge-slim ${stale ? "is-stale" : ""}`}>
        <span className="provider-glyph" aria-hidden="true"><ProviderLogo id={snapshot.id} glyph={snapshot.glyph} /></span>
        <div
          aria-hidden="true"
          style={{
            position: "absolute",
            left: 5,
            right: 5,
            bottom: 4,
            height: 4,
            borderRadius: 999,
            overflow: "hidden",
            background: "var(--track)",
          }}
        >
          {used != null && (
            <span
              style={{
                display: "block",
                width: `${fraction * 100}%`,
                height: "100%",
                borderRadius: 999,
                background: color,
                transition: "width 420ms cubic-bezier(.22,.8,.2,1), background 220ms ease",
              }}
            />
          )}
        </div>
        <ActivityOverlay snapshot={snapshot} />
      </div>
    );
  }

  if (gaugeStyle === "halo") {
    const activeTicks = used == null ? 0 : Math.round(fraction * 12);
    return (
      <div className={`provider-ring gauge-halo ${stale ? "is-stale" : ""}`}>
        <svg className="ring-svg" viewBox="0 0 44 44" aria-hidden="true">
          {Array.from({ length: 12 }, (_, index) => {
            const active = index < activeTicks;
            return (
              <line
                key={index}
                x1="22"
                y1="3.6"
                x2="22"
                y2="8.1"
                stroke={active ? color : "var(--track)"}
                strokeWidth={active ? 2.8 : 2.2}
                strokeLinecap="round"
                opacity={active ? 1 : 0.72}
                transform={`rotate(${index * 30} 22 22)`}
                style={active ? { filter: `drop-shadow(0 0 2.5px ${color})` } : undefined}
              />
            );
          })}
        </svg>
        <span className="provider-glyph" aria-hidden="true"><ProviderLogo id={snapshot.id} glyph={snapshot.glyph} /></span>
        <ActivityOverlay snapshot={snapshot} />
      </div>
    );
  }

  if (gaugeStyle === "stacked") {
    return (
      <div className={`provider-ring gauge-stacked ${stale ? "is-stale" : ""}`}>
        <span
          aria-hidden="true"
          style={{ position: "absolute", top: 1, left: 0, right: 0, height: 18, display: "grid", placeItems: "center", color: "var(--text)" }}
        >
          <ProviderLogo id={snapshot.id} glyph={snapshot.glyph} size={15} />
        </span>
        <span style={{ position: "absolute", left: 3, right: 3, bottom: 3, display: "grid", gap: 2.5 }} aria-hidden="true">
          {windows.length ? windows.map((window) => (
            <span key={window.id} style={{ display: "grid", gridTemplateColumns: "7px 1fr", alignItems: "center", gap: 2 }}>
              <span style={{ color: "var(--faint)", fontSize: 6.5, lineHeight: 1, fontWeight: 750, textAlign: "center" }}>{compactLabel(window)}</span>
              <GaugeBar window={window} surface={surface} height={3} />
            </span>
          )) : <span style={{ height: 3, borderRadius: 999, background: "var(--track)" }} />}
        </span>
        <ActivityOverlay snapshot={snapshot} />
      </div>
    );
  }

  if (gaugeStyle === "columns") {
    return (
      <div className={`provider-ring gauge-columns ${stale ? "is-stale" : ""}`}>
        <span
          aria-hidden="true"
          style={{ position: "absolute", top: 1, left: 0, right: 0, height: 17, display: "grid", placeItems: "center", color: "var(--text)" }}
        >
          <ProviderLogo id={snapshot.id} glyph={snapshot.glyph} size={14} />
        </span>
        <span aria-hidden="true" style={{ position: "absolute", left: 8, right: 8, bottom: 3, height: 20, display: "flex", alignItems: "flex-end", justifyContent: "center", gap: 4 }}>
          {(windows.length ? windows : [{ id: "empty", label: "—", usedFraction: 1 }]).map((window) => {
            const value = remainingFraction(window.usedFraction);
            return (
              <span key={window.id} title={`${window.label}: ${formatPercent(value)} remaining`} style={{ width: 5, height: 18, borderRadius: 999, background: "var(--track)", overflow: "hidden", display: "flex", alignItems: "flex-end" }}>
                <span style={{ width: "100%", height: `${Math.max(value * 100, value > 0 ? 8 : 0)}%`, borderRadius: 999, background: bandForRemaining(surface, value), transition: "height 420ms cubic-bezier(.22,.8,.2,1), background 220ms ease" }} />
              </span>
            );
          })}
        </span>
        <ActivityOverlay snapshot={snapshot} />
      </div>
    );
  }

  if (gaugeStyle === "micro") {
    return (
      <div className={`provider-ring gauge-micro ${stale ? "is-stale" : ""}`}>
        <span aria-hidden="true" style={{ position: "absolute", left: 2, top: 0, bottom: 0, width: 16, display: "grid", placeItems: "center", color: "var(--text)" }}>
          <ProviderLogo id={snapshot.id} glyph={snapshot.glyph} size={13} />
        </span>
        <span aria-hidden="true" style={{ position: "absolute", left: 20, right: 2, top: "50%", transform: "translateY(-50%)", display: "grid", gap: 3 }}>
          {windows.length ? windows.map((window) => <GaugeBar key={window.id} window={window} surface={surface} height={2.5} />) : <span style={{ height: 2.5, borderRadius: 999, background: "var(--track)" }} />}
        </span>
        <ActivityOverlay snapshot={snapshot} />
      </div>
    );
  }

  return (
    <div className={`provider-ring gauge-classic ${stale ? "is-stale" : ""}`}>
      <svg className="ring-svg" viewBox="0 0 44 44" aria-hidden="true">
        <circle className="ring-track" cx="22" cy="22" r={classicRadius} strokeWidth={classicTrackStroke} />
        {used != null && (
          <circle
            className="ring-value"
            cx="22"
            cy="22"
            r={classicRadius}
            stroke={color}
            strokeWidth={classicProgressStroke}
            strokeDasharray={classicCircumference}
            strokeDashoffset={classicCircumference * (1 - fraction)}
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
  gaugeStyle,
  shellStyle,
  hasResetAlert,
  onHover,
  onLeave,
  onOpen,
}: {
  snapshot: ProviderSnapshot;
  index: number;
  edge: Edge;
  surface: string;
  gaugeStyle: GaugeStyle;
  shellStyle: ShellStyle;
  hasResetAlert: boolean;
  onHover: (snapshot: ProviderSnapshot, index: number) => void;
  onLeave: () => void;
  onOpen: (snapshot: ProviderSnapshot) => void;
}) {
  const headlineUsed = headlineWindow(snapshot)?.usedFraction;
  const headlineRemaining = headlineUsed == null ? null : remainingFraction(headlineUsed);
  const compact = isCompactGauge(gaugeStyle);
  const usageSummary = snapshot.windows.length
    ? snapshot.windows.slice(0, 3).map((window) => `${window.label} ${formatPercent(remainingFraction(window.usedFraction))} remaining`).join(", ")
    : headlineRemaining == null ? "usage unavailable" : `${formatPercent(headlineRemaining)} remaining`;
  const slide = edge === "right" || edge === "left"
    ? { x: 0, y: 12 }
    : { x: 12, y: 0 };
  const cellStyle: CSSProperties = compact
    ? { position: "relative", width: 44, minWidth: 44, height: 44, gap: 0 }
    : { position: "relative" };
  return (
    <motion.button
      type="button"
      className={`provider-cell${compact ? " is-compact" : ""}`}
      style={cellStyle}
      aria-label={`${snapshot.displayName} ${usageSummary}${hasResetAlert ? ", quota refreshed" : ""}`}
      onMouseEnter={() => onHover(snapshot, index)}
      onMouseLeave={onLeave}
      onFocus={() => onHover(snapshot, index)}
      onBlur={onLeave}
      onClick={() => onOpen(snapshot)}
      data-edge={edge}
      initial={{ opacity: 0, scale: 0.8, ...slide }}
      animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, transition: { duration: 0.15 } }}
      whileHover={{ scale: isDockShell(shellStyle) ? 1 : 1.07 }}
      whileTap={{ scale: 0.94 }}
      transition={{ type: "spring", stiffness: 420, damping: 26, delay: Math.min(index * 0.05, 0.25) }}
    >
      <ProviderGauge snapshot={snapshot} surface={surface} gaugeStyle={gaugeStyle} />
      {hasResetAlert && (
        <motion.span
          aria-hidden="true"
          style={{
            position: "absolute",
            zIndex: 6,
            top: -2,
            right: -1,
            width: 9,
            height: 9,
            borderRadius: "50%",
            background: resetGreen,
            boxShadow: "0 0 9px rgba(0,255,136,.95)",
            pointerEvents: "none",
          }}
          animate={{ opacity: [1, 0.42, 1] }}
          transition={{ duration: 1.5, repeat: Infinity, ease: "easeInOut" }}
        />
      )}
      {!compact && <span className="provider-percent">{headlineRemaining == null ? (snapshot.displayValue ?? "—") : formatPercent(headlineRemaining)}</span>}
    </motion.button>
  );
}

export function NotchView() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [settings, setSettings] = useState(loadSettings);
  const [usageAlerts, setUsageAlerts] = useState(getUsageResetAlerts);
  const [resetReveal, setResetReveal] = useState<UsageResetAlert | null>(null);
  const [hoveredId, setHoveredId] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [retracted, setRetracted] = useState(false);
  const [autoHideAvailable, setAutoHideAvailable] = useState(false);
  const retractedRef = useRef(false);
  const settingsRef = useRef(settings);
  const resetRevealTimer = useRef<number | null>(null);

  retractedRef.current = retracted;
  settingsRef.current = settings;

  useEffect(() => {
    let live = true;
    void checkForUpdates()
      .then((info) => { if (live) setUpdateAvailable(info?.available ?? false); })
      .catch(() => undefined);
    return () => { live = false; };
  }, []);

  useEffect(() => {
    let live = true;
    void autohideSupported().then((supported) => {
      if (live) setAutoHideAvailable(supported);
    });
    return () => { live = false; };
  }, []);

  const sysLight = useSystemLight();
  const surface = resolveSurface(settings.mode, settings.surface, sysLight, settings.theme);
  const shellStyle = { ...themeVars(surface), zoom: settings.scale, opacity: settings.opacity };
  const leaveTimer = useRef<number | null>(null);
  const leaveGen = useRef(0);
  const autoHideTimer = useRef<number | null>(null);
  const autoHideGen = useRef(0);
  const retractGen = useRef(0);
  const tipRef = useRef<{ id: string; at: number } | null>(null);
  const edge = settings.edge;
  const compactGauge = isCompactGauge(settings.gaugeStyle);
  const dockShell = isDockShell(settings.shellStyle);

  const enabled = useMemo(
    () => snapshots.filter((item) => settings.enabledProviders.includes(item.id)),
    [settings, snapshots],
  );
  const activeUsageAlerts = useMemo(
    () => usageAlerts.filter((alert) => settings.enabledProviders.includes(alert.providerId)),
    [settings.enabledProviders, usageAlerts],
  );
  const alertedProviders = useMemo(
    () => new Set(activeUsageAlerts.map((alert) => alert.providerId)),
    [activeUsageAlerts],
  );
  const resetRevealSnapshot = resetReveal
    ? snapshots.find((snapshot) => snapshot.id === resetReveal.providerId)
    : undefined;

  const refresh = useCallback(async () => {
    try {
      const previousAlertIds = new Set(getUsageResetAlerts().map((alert) => alert.id));
      const next = await getSnapshots();
      setSnapshots(next);
      const nextAlerts = processUsageResetSnapshots(next);
      setUsageAlerts(nextAlerts);

      const newest = [...nextAlerts]
        .reverse()
        .find((alert) => !previousAlertIds.has(alert.id) && settingsRef.current.enabledProviders.includes(alert.providerId));
      if (newest && retractedRef.current) {
        setResetReveal(newest);
        if (resetRevealTimer.current != null) window.clearTimeout(resetRevealTimer.current);
        resetRevealTimer.current = window.setTimeout(() => {
          resetRevealTimer.current = null;
          setResetReveal(null);
        }, 2400);
      }
    } catch {
      // Keep the last good readings rather than blanking the notch.
    }
  }, []);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 60_000);
    return () => window.clearInterval(id);
  }, [refresh]);

  const cancelAutoHide = useCallback(() => {
    autoHideGen.current++;
    if (autoHideTimer.current != null) {
      window.clearTimeout(autoHideTimer.current);
      autoHideTimer.current = null;
    }
  }, []);

  const peek = useCallback(async () => {
    cancelAutoHide();
    const gen = ++retractGen.current;
    await setNotchRetracted(false, edge);
    if (gen !== retractGen.current) return;
    setRetracted(false);
  }, [cancelAutoHide, edge]);

  const retract = useCallback(() => {
    if (!settings.autoHide || !autoHideAvailable || retracted) return;
    retractGen.current++;
    setRetracted(true);
  }, [autoHideAvailable, retracted, settings.autoHide]);

  const scheduleAutoHide = useCallback(() => {
    cancelAutoHide();
    if (!settings.autoHide || !autoHideAvailable || retracted) return;
    const gen = ++autoHideGen.current;

    const arm = (delayMs: number) => {
      autoHideTimer.current = window.setTimeout(() => {
        autoHideTimer.current = null;
        void (async () => {
          const overOverlay = await cursorOverOverlay();
          if (gen !== autoHideGen.current) return;
          if (overOverlay === true) {
            arm(1000);
            return;
          }
          retract();
        })();
      }, delayMs);
    };

    arm(settings.autoHideDelaySec * 1000);
  }, [autoHideAvailable, cancelAutoHide, retract, retracted, settings.autoHide, settings.autoHideDelaySec]);

  useEffect(() => {
    leaveGen.current++;
    tipRef.current = null;
    void hideTooltip();
    cancelAutoHide();
    const gen = ++retractGen.current;
    void (async () => {
      await setNotchRetracted(false, edge);
      if (gen !== retractGen.current) return;
      setRetracted(false);
      await setEdge(
        edge,
        Math.max(enabled.length, 1),
        settings.scale,
        settings.monitor,
        settings.offsetX,
        settings.offsetY,
        compactGauge,
        settings.shellStyle,
      );
    })();
  }, [cancelAutoHide, compactGauge, edge, enabled.length, settings.scale, settings.monitor, settings.offsetX, settings.offsetY, settings.shellStyle]);

  useEffect(() => {
    if (!settings.autoHide && retracted) void peek();
  }, [peek, retracted, settings.autoHide]);

  useEffect(() => () => {
    cancelAutoHide();
    if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    if (resetRevealTimer.current != null) window.clearTimeout(resetRevealTimer.current);
  }, [cancelAutoHide]);

  const doHide = useCallback(() => {
    tipRef.current = null;
    setHoveredId(null);
    void hideTooltip();
  }, []);

  const leave = useCallback(() => {
    const gen = ++leaveGen.current;
    if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    leaveTimer.current = window.setTimeout(() => {
      void (async () => {
        try {
          const inside = await cursorOverTooltipArea();
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
    void listen("settings:closed", () => scheduleAutoHide()).then((fn) => unlisten.push(fn));
    void listen("notch:peek", () => { void peek(); }).then((fn) => unlisten.push(fn));
    void listen("tooltip:hover", () => {
      if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    }).then((fn) => unlisten.push(fn));
    void listen("tooltip:leave", () => {
      leave();
    }).then((fn) => unlisten.push(fn));
    return () => unlisten.forEach((fn) => fn());
  }, [refresh, leave, peek, scheduleAutoHide]);

  const hover = (snapshot: ProviderSnapshot, index: number) => {
    leaveGen.current++;
    if (leaveTimer.current != null) window.clearTimeout(leaveTimer.current);
    void trace(`enter ${snapshot.id} ${index}`);
    if (tipRef.current?.id === snapshot.id) return;
    tipRef.current = { id: snapshot.id, at: Date.now() };
    setHoveredId(snapshot.id);
    if (runningInTauri()) {
      void emitTo("tooltip", "tooltip:show", { snapshot, edge, index });
    }
    void showTooltip(edge, index, settings.scale, compactGauge, settings.shellStyle).catch(() => {
      tipRef.current = null;
    });
  };

  const openSnapshot = (snapshot: ProviderSnapshot) => {
    setUsageAlerts(clearUsageResetAlerts(snapshot.id));
    if (resetReveal?.providerId === snapshot.id) setResetReveal(null);
    void openProvider(snapshot);
  };

  const enterFrom =
    edge === "right" ? { x: 36, y: 0 }
    : edge === "left" ? { x: -36, y: 0 }
    : edge === "top" ? { x: 0, y: -36 }
    : { x: 0, y: 36 };

  const depth = shellDepth(settings.shellStyle, edge, compactGauge);
  const hideDistance = Math.max(depth - 6, 0);
  const hideOffset =
    edge === "right" ? { x: hideDistance, y: 0 }
    : edge === "left" ? { x: -hideDistance, y: 0 }
    : edge === "top" ? { x: 0, y: -hideDistance }
    : { x: 0, y: hideDistance };
  const animationGen = retractGen.current;

  const handleShellContextMenu = (event: MouseEvent<HTMLElement>) => {
    event.preventDefault();
    void showContextMenu(edge, settings.scale);
  };

  const handleShellMouseEnter = () => {
    cancelAutoHide();
    if (retracted) {
      void peek();
    } else {
      retractGen.current++;
    }
  };

  const handleShellMouseLeave = () => {
    setHoveredId(null);
    scheduleAutoHide();
  };

  const shellMotionProps: ComponentProps<typeof motion.main> = {
    className: `notch-shell edge-${edge}`,
    style: shellStyle,
    initial: { opacity: 0, ...enterFrom },
    animate: {
      opacity: settings.opacity,
      x: retracted ? hideOffset.x : 0,
      y: retracted ? hideOffset.y : 0,
    },
    transition: { type: "spring", stiffness: 210, damping: 26, opacity: { duration: 0.18 } },
    onAnimationComplete: () => {
      if (!retracted || animationGen !== retractGen.current || !settings.autoHide || !autoHideAvailable) return;
      void setNotchRetracted(true, edge);
    },
    onContextMenu: handleShellContextMenu,
    onMouseEnter: handleShellMouseEnter,
    onMouseLeave: handleShellMouseLeave,
  };

  const resetSliver = activeUsageAlerts.length > 0 ? (
    <motion.span
      aria-hidden="true"
      style={resetSliverStyle(edge)}
      animate={{ opacity: [1, 0.4, 1] }}
      transition={{ duration: 1.5, repeat: Infinity, ease: "easeInOut" }}
    />
  ) : null;

  const resetPeek = retracted && resetReveal && resetRevealSnapshot ? (
    <motion.span
      key={resetReveal.id}
      aria-hidden="true"
      style={resetPeekStyle(edge)}
      initial={{ opacity: 0, scale: 0.72 }}
      animate={{
        ...resetPeekMotion(edge),
        opacity: [0, 1, 1, 1, 0],
        scale: [0.72, 1.1, 1, 1, 0.82],
      }}
      transition={{ duration: 2.15, times: [0, 0.18, 0.32, 0.78, 1], ease: "easeInOut" }}
    >
      <ProviderLogo id={resetRevealSnapshot.id} glyph={resetRevealSnapshot.glyph} size={18} />
      <span
        style={{
          position: "absolute",
          right: 2,
          top: 2,
          width: 7,
          height: 7,
          borderRadius: "50%",
          background: resetGreen,
          boxShadow: "0 0 7px rgba(0,255,136,.95)",
        }}
      />
    </motion.span>
  ) : null;

  if (!enabled.length) {
    return (
      <motion.main {...shellMotionProps}>
        {resetSliver}
        {resetPeek}
        <button className="settings-orb empty" type="button" onClick={() => void openSettings()} aria-label="Settings">
          <svg className="orb-cog" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06-.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09A1.65 1.65 0 0 0 19.4 15z" />
          </svg>
        </button>
      </motion.main>
    );
  }

  const stackStyle: CSSProperties | undefined = compactGauge && !dockShell
    ? (edge === "right" || edge === "left"
      ? { gap: 12, padding: "16px 11px 14px" }
      : { gap: 12, padding: "7px 14px 8px 16px" })
    : undefined;

  return (
    <motion.main {...shellMotionProps}>
      {resetSliver}
      {resetPeek}
      <div key={`${edge}-${enabled.length}-${settings.gaugeStyle}-${settings.shellStyle}`} className="provider-stack" style={stackStyle}>
        {enabled.map((snapshot, index) => (
          <ProviderCell
            key={snapshot.id}
            snapshot={snapshot}
            index={index}
            edge={edge}
            surface={surface}
            gaugeStyle={settings.gaugeStyle}
            shellStyle={settings.shellStyle}
            hasResetAlert={alertedProviders.has(snapshot.id)}
            onHover={hover}
            onLeave={leave}
            onOpen={openSnapshot}
          />
        ))}
      </div>
      <button className={`settings-orb${hoveredId ? " peek" : ""}`} type="button" onClick={() => void openSettings()} aria-label="Settings">
        {updateAvailable && <span className="orb-update" aria-label="Update available" />}
        <svg className="orb-cog" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v-.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06-.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09A1.65 1.65 0 0 0 19.4 15z" />
        </svg>
      </button>
    </motion.main>
  );
}
