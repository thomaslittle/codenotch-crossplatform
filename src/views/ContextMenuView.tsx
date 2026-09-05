import { appAction, hideWindow } from "../lib/backend";

const actions = [
  ["settings", "Settings…"],
  ["refresh", "Refresh now"],
  ["hide-hour", "Hide for 1 hour"],
  ["quit", "Quit"],
] as const;

export function ContextMenuView() {
  return (
    <main className="context-menu">
      {actions.map(([action, label]) => (
        <button key={action} type="button" onClick={() => {
          void appAction(action);
          void hideWindow("context-menu");
        }}>{label}</button>
      ))}
    </main>
  );
}
