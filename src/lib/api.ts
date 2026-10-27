import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  AiStatus,
  Answer,
  BuildInfo,
  BuildOptions,
  BuildResult,
  Category,
  EditState,
  Explanation,
  ModelInfo,
  Proposal,
  ValueChange,
  GameProject,
  ImportProgress,
  ImportRecord,
  Preview,
  Settings,
  ToolStatus,
} from "./types";

/** False when the UI is opened in a plain browser (e.g. `npm run dev`). */
export const inDesktopApp = isTauri();

export const api = {
  importGame: (path: string) => invoke<ImportRecord>("import_game", { path }),
  listProjects: () => invoke<ImportRecord[]>("list_projects"),
  getProject: (id: string) => invoke<ImportRecord>("get_project", { id }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  getAnalysis: (id: string, refresh = false) =>
    invoke<GameProject>("get_analysis", { id, refresh }),
  /** `null` restores the analyzer's choice. */
  setCategory: (id: string, objectId: string, category: Category | null) =>
    invoke<void>("set_category", { id, objectId, category }),
  readPreview: (id: string, path: string) => invoke<Preview>("read_preview", { id, path }),
  readTextFile: (id: string, path: string) => invoke<string>("read_text_file", { id, path }),
  getEditState: (id: string) => invoke<EditState>("get_edit_state", { id }),
  saveTextFile: (id: string, path: string, text: string) =>
    invoke<EditState>("save_text_file", { id, path, text }),
  replaceFile: (id: string, path: string, source: string) =>
    invoke<EditState>("replace_file", { id, path, source }),
  revertFile: (id: string, path: string) => invoke<EditState>("revert_file", { id, path }),
  undoEdit: (id: string) => invoke<EditState>("undo_edit", { id }),
  getBuildInfo: (id: string) => invoke<BuildInfo>("get_build_info", { id }),
  buildApk: (id: string, options: BuildOptions) => invoke<BuildResult>("build_apk", { id, options }),
  readPng: (path: string) => invoke<string>("read_png", { path }),
  aiStatus: () => invoke<AiStatus>("ai_status"),
  aiSetKey: (key: string) => invoke<{ status: AiStatus; models: ModelInfo[] }>("ai_set_key", { key }),
  aiModels: () => invoke<ModelInfo[]>("ai_models"),
  aiSetModel: (model: string) => invoke<AiStatus>("ai_set_model", { model }),
  aiClearKey: () => invoke<AiStatus>("ai_clear_key"),
  aiExplain: (id: string, path: string, selection: string | null) =>
    invoke<Explanation>("ai_explain", { id, path, selection }),
  aiAsk: (id: string, path: string, selection: string | null, question: string) =>
    invoke<Answer>("ai_ask", { id, path, selection, question }),
  aiPropose: (id: string, path: string, selection: string | null, changes: ValueChange[], instruction: string) =>
    invoke<Proposal>("ai_propose", { id, path, selection, changes, instruction }),
  onBuildProgress: (handler: (p: ImportProgress) => void): Promise<UnlistenFn> =>
    listen<ImportProgress>("build-progress", (e) => handler(e.payload)),
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

/** Opens a file picker limited to the given extensions. */
export async function pickFile(label: string, extensions: string[]): Promise<string | null> {
  const result = await open({ multiple: false, filters: [{ name: label, extensions }] });
  return typeof result === "string" ? result : null;
}

/** Asks where to save a new APK. */
export async function pickSaveApk(defaultName: string): Promise<string | null> {
  return save({ defaultPath: defaultName, filters: [{ name: "Android package", extensions: ["apk"] }] });
}

/** True for paths GameX can edit: apktool output (rebuilt) or plain imported files. */
export function isEditablePath(path: string): boolean {
  return path.startsWith("apktool/") || path.startsWith("files/");
}

export function extensionOf(path: string): string {
  const name = path.split("/").pop() ?? path;
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
}

/** Formats replaced with a picked file rather than edited as text. */
export const mediaExtensions = new Set([
  "png", "jpg", "jpeg", "webp", "gif", "bmp", "ogg", "mp3", "wav", "m4a", "aac", "flac", "ttf", "otf", "mp4",
]);

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
