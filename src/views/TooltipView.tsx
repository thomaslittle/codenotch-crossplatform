import { emitTo, listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "motion/react";
import { useEffect, useState } from "react";
import { formatPercent, formatReset } from "../lib/usage";
import { runningInTauri } from "../lib/backend";
import { loadSettings } from "../lib/settings";
import { bandFor, resolveSurface, themeVars, useSystemLight } from "../lib/theme";
import { ProviderLogo } from "../components/ProviderLogo";
import type { Edge, ProviderSnapshot } from "../types";

type Payload = { snapshot: ProviderSnapshot; edge: Edge; index: number };

function slideFor(edge: Edge): { x: number; y: number } {
  switch (edge) {
    case "right": return { x: 10, y: 0 };
    case "left": return { x: -10, y: 0 };
    case "top": return { x: 0, y: -10 };
    case "bottom": return { x: 0, y: 10 };
  }
}

export function TooltipView() {
  const [payload, setPayload] = useState<Payload | null>(null);
  const [settings, setSettings] = useState(loadSettings);
  const sysLight = useSystemLight();
  const surface = resolveSurface(settings.mode, settings.surface, sysLight);

  useEffect(() => {
    if (!runningInTauri()) return;
    const unlisten: Array<() => void> = [];
    void listen<Payload>("tooltip:show", (event) => setPayload(event.payload)).then((fn) => unlisten.push(fn));
    void listen("settings:changed", () => setSettings(loadSettings())).then((fn) => unlisten.push(fn));
    return () => unlisten.forEach((fn) => fn());
  }, []);

  if (!payload) return <main className="tooltip-stage" />;
  const { snapshot, edge } = payload;
  const slide = slideFor(edge);

  return (
    <main
      className={`tooltip-stage tooltip-${edge}`}
      style={themeVars(surface, settings.opacity)}
      onMouseEnter={() => runningInTauri() && void emitTo("notch", "tooltip:hover")}
      onMouseLeave={() => runningInTauri() && void emitTo("notch", "tooltip:leave")}
    >
      <AnimatePresence mode="wait" initial={false}>
        <motion.section
          key={snapshot.id}
          className="usage-card"
          aria-label={`${snapshot.displayName} usage details`}
          initial={{ opacity: 0, scale: 0.92, ...slide }}
          animate={{ opacity: 1, scale: 1, x: 0, y: 0 }}
          exit={{ opacity: 0, scale: 0.95, transition: { duration: 0.12 } }}
          transition={{ type: "spring", stiffness: 480, damping: 30 }}
        >
        <header className="card-header">
          <span className="card-glyph"><ProviderLogo id={snapshot.id} glyph={snapshot.glyph} size={15} /></span>
          <strong>{snapshot.displayName} Usage</strong>
          {snapshot.fidelity !== "official" && <span className="fidelity-badge">~ {snapshot.fidelity}</span>}
        </header>
        {snapshot.windows.length ? snapshot.windows.map((window, windowIndex) => (
          <motion.div
            className="limit-block"
            key={window.id}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ duration: 0.2, delay: 0.05 + windowIndex * 0.05, ease: "easeOut" }}
          >
            <div className="limit-line">
              <span>{window.label}</span>
              <span className="reset-copy">{formatReset(window.resetsAt)}</span>
            </div>
            <div className="limit-track" aria-hidden="true">
              <motion.span
                className="limit-fill"
                initial={{ width: 0 }}
                animate={{ width: `${Math.min(100, Math.max(0, window.usedFraction * 100))}%` }}
                transition={{ type: "spring", stiffness: 90, damping: 20, delay: 0.08 + windowIndex * 0.05 }}
                style={{ background: bandFor(surface, window.usedFraction) }}
              />
            </div>
            <span className="used-copy">{formatPercent(window.usedFraction)} Used</span>
          </motion.div>
        )) : (
          <p className="status-copy">{snapshot.message ?? "No usage window is available yet."}</p>
        )}
        {snapshot.activity && snapshot.activity.state !== "idle" && (
          <div className={`activity-row state-${snapshot.activity.state}`}>
            <span className="activity-dot" />
            <span>{snapshot.activity.label ?? (snapshot.activity.state === "working" ? "Working now" : "Waiting on you")}</span>
          </div>
        )}
        {snapshot.status !== "ok" && <p className="status-copy">{snapshot.message ?? snapshot.status}</p>}
        </motion.section>
      </AnimatePresence>
    </main>
  );
}
