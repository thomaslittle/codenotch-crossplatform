import type { ClientSettings, Edge } from "../types";

const STORAGE_KEY = "codenotch-crossplatform.settings.v1";

export const DEFAULT_SETTINGS: ClientSettings = {
  edge: "right",
  enabledProviders: ["claude", "cursor", "codex", "gemini"],
};

export function loadSettings(): ClientSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_SETTINGS;
    const parsed = JSON.parse(raw) as Partial<ClientSettings>;
    const edge = isEdge(parsed.edge) ? parsed.edge : DEFAULT_SETTINGS.edge;
    const enabledProviders = Array.isArray(parsed.enabledProviders)
      ? parsed.enabledProviders.filter((value): value is string => typeof value === "string")
      : DEFAULT_SETTINGS.enabledProviders;
    return { edge, enabledProviders };
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export function saveSettings(settings: ClientSettings): void {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
}

function isEdge(value: unknown): value is Edge {
  return value === "right" || value === "left" || value === "top" || value === "bottom";
}
