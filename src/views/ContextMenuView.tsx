import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { appAction, hideWindow, runningInTauri } from "../lib/backend";
import { listen } from "@tauri-apps/api/event";
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
  const sysLight = useSystemLight();
  const surface = resolveSurface(settings.mode, settings.surface, sysLight);

  useEffect(() => {
    if (!runningInTauri()) return;
    let dispose: (() => void) | undefined;
    void listen("settings:changed", () => setSettings(loadSettings())).then((fn) => { dispose = fn; });
    return () => dispose?.();
  }, []);

  return (
    <motion.main
      className="context-menu"
      style={themeVars(surface, settings.opacity)}
      initial={{ opacity: 0, scale: 0.95, y: -4 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
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
