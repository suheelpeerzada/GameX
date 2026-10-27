import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type { ImportProgress, ImportRecord, Settings, ToolStatus } from "./types";

/** False when the UI is opened in a plain browser (e.g. `npm run dev`). */
export const inDesktopApp = isTauri();

export const api = {
  importGame: (path: string) => invoke<ImportRecord>("import_game", { path }),
  listProjects: () => invoke<ImportRecord[]>("list_projects"),
  getProject: (id: string) => invoke<ImportRecord>("get_project", { id }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  toolStatus: () => invoke<ToolStatus[]>("tool_status"),
  onImportProgress: (handler: (p: ImportProgress) => void): Promise<UnlistenFn> =>
    listen<ImportProgress>("import-progress", (e) => handler(e.payload)),
};

export type PickKind = "apk" | "zip" | "folder" | "tool";

/** Opens the native picker. Resolves to null when the user cancels. */
export async function pickPath(kind: PickKind): Promise<string | null> {
  const filters = {
    apk: [{ name: "Android package", extensions: ["apk"] }],
    zip: [{ name: "ZIP archive", extensions: ["zip"] }],
    folder: undefined,
    tool: undefined,
  }[kind];
  const result = await open({ multiple: false, directory: kind === "folder", filters });
  return typeof result === "string" ? result : null;
}

export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export const containerLabel = { apk: "APK", zip: "ZIP", folder: "Folder" } as const;
