import { emitTo, listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { bandColor, formatPercent, formatReset } from "../lib/usage";
import { runningInTauri } from "../lib/backend";
import type { Edge, ProviderSnapshot } from "../types";

type Payload = { snapshot: ProviderSnapshot; edge: Edge; index: number };

export function TooltipView() {
  const [payload, setPayload] = useState<Payload | null>(null);

  useEffect(() => {
    if (!runningInTauri()) return;
    let dispose: (() => void) | undefined;
    void listen<Payload>("tooltip:show", (event) => setPayload(event.payload)).then((fn) => { dispose = fn; });
    return () => dispose?.();
  }, []);

  if (!payload) return <main className="tooltip-stage" />;
  const { snapshot, edge } = payload;

  return (
    <main
      className={`tooltip-stage tooltip-${edge}`}
      onMouseEnter={() => runningInTauri() && void emitTo("notch", "tooltip:hover")}
      onMouseLeave={() => runningInTauri() && void emitTo("notch", "tooltip:leave")}
    >
      <section className="usage-card" aria-label={`${snapshot.displayName} usage details`}>
        <header className="card-header">
          <span className="card-glyph">{snapshot.glyph}</span>
          <strong>{snapshot.displayName} Usage</strong>
          {snapshot.fidelity !== "official" && <span className="fidelity-badge">~ {snapshot.fidelity}</span>}
        </header>
        {snapshot.windows.length ? snapshot.windows.map((window) => (
          <div className="limit-block" key={window.id}>
            <div className="limit-line">
              <span>{window.label}</span>
              <span className="reset-copy">{formatReset(window.resetsAt)}</span>
            </div>
            <div className="limit-track" aria-hidden="true">
              <span
                className="limit-fill"
                style={{ width: `${Math.min(100, Math.max(0, window.usedFraction * 100))}%`, background: bandColor(window.usedFraction) }}
              />
            </div>
            <span className="used-copy">{formatPercent(window.usedFraction)} Used</span>
          </div>
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
      </section>
    </main>
  );
}
