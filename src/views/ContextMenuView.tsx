import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { appAction, hideWindow, runningInTauri } from "../lib/backend";
import { loadSettings } from "../lib/settings";
import { resolveSurface, themeVars, useSystemLight } from "../lib/theme";

const actions = [
  ["settings", "Settings…"],
  ["refresh", "Refresh now"],
  ["hide-hour", "Hide for 1 hour"],
  ["quit", "Quit"],
] as const;

export function ContextMenuView() {
  const [settings, setSettings] = useState(loadSettings);
  const menuRef = useRef<HTMLElement | null>(null);
  const sysLight = useSystemLight();
  const surface = resolveSurface(settings.mode, settings.surface, sysLight, settings.theme);

  useEffect(() => {
    if (!runningInTauri()) return;
    let disposed = false;
    const cleanup: Array<() => void> = [];

    const dismiss = () => {
      void hideWindow("context-menu");
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      dismiss();
    };

    const onPointerDown = (event: PointerEvent) => {
      if (menuRef.current?.contains(event.target as Node)) return;
      dismiss();
    };

    window.addEventListener("keydown", onKeyDown);
    document.addEventListener("pointerdown", onPointerDown, true);
    cleanup.push(() => window.removeEventListener("keydown", onKeyDown));
    cleanup.push(() => document.removeEventListener("pointerdown", onPointerDown, true));

    void listen("settings:changed", () => setSettings(loadSettings())).then((fn) => {
      if (disposed) fn();
      else cleanup.push(fn);
    });

    void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (!focused) dismiss();
    }).then((fn) => {
      if (disposed) fn();
      else cleanup.push(fn);
    });

    return () => {
      disposed = true;
      cleanup.splice(0).forEach((fn) => fn());
    };
  }, []);

  return (
    <motion.main
      ref={menuRef}
      className="context-menu"
      style={{ ...themeVars(surface), opacity: settings.opacity }}
      initial={{ scale: 0.95, y: -4 }}
      animate={{ scale: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 500, damping: 32 }}
    >
      {actions.map(([action, label]) => (
        <button key={action} type="button" onClick={() => {
          void appAction(action);
          void hideWindow("context-menu");
        }}>{label}</button>
      ))}
    </motion.main>
  );
}
