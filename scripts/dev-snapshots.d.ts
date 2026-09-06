declare module "../scripts/dev-snapshots.mjs" {
  import type { Plugin } from "vite";
  import type { ProviderSnapshot } from "../src/types";
  export function devSnapshots(): Promise<ProviderSnapshot[]>;
  export function devSnapshotsPlugin(): Plugin;
}
