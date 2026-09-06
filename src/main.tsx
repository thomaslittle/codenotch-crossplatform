import { MotionConfig } from "motion/react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";
import "./shells.css";
import { ContextMenuView } from "./views/ContextMenuView";
import { NotchView } from "./views/NotchView";
import { SettingsView } from "./views/SettingsView";
import { TooltipView } from "./views/TooltipView";

function Root() {
  const view = new URLSearchParams(window.location.search).get("view") ?? "notch";

  if (view === "tooltip") return <TooltipView />;
  if (view === "settings") return <SettingsView />;
  if (view === "context") return <ContextMenuView />;
  return <NotchView />;
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <MotionConfig reducedMotion="user">
      <Root />
    </MotionConfig>
  </StrictMode>,
);
