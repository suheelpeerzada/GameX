import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  AiStatus,
  Answer,
  BuildInfo,
  ToolInfo,
  ToolProgress,
  RunOptions,
  RunStatus,
  RunTarget,
  Launched,
  LiveClassCount,
  LiveInstance,
  LiveObject,
  BuildOptions,
  BuildResult,
  Category,
  EditState,
  Explanation,
  ModelInfo,
  Proposal,
  References,
  MediaItem,
  Scene,
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
  getReferences: (id: string, path: string) => invoke<References>("get_references", { id, path }),
  scanMedia: (id: string, path: string) => invoke<MediaItem[]>("scan_media", { id, path }),
  sceneLoad: (id: string, path: string) => invoke<Scene>("scene_load", { id, path }),
  /** Sets properties of one node as one undoable edit. */
  sceneSet: (id: string, path: string, node: string, changes: [string, string][]) =>
    invoke<EditState>("scene_set", { id, path, node, changes }),
  exportFiles: (id: string, dest: string) => invoke<number>("export_files", { id, dest }),
  readMediaItem: (id: string, path: string, offset: number, len: number, mime: string) =>
    invoke<string>("read_media_item", { id, path, offset, len, mime }),
  saveMediaItem: (id: string, path: string, offset: number, len: number, dest: string) =>
    invoke<void>("save_media_item", { id, path, offset, len, dest }),
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
  toolsCatalog: () => invoke<ToolInfo[]>("tools_catalog"),
  runTargets: () => invoke<RunTarget[]>("run_targets"),
  runEnableAdb: (index: number) => invoke<void>("run_enable_adb", { index }),
  runOptions: (id: string) => invoke<RunOptions>("run_options", { id }),
  runGame: (id: string, target: string, apk: "original" | "build", replace: boolean) =>
    invoke<Launched>("run_game", { id, target, apk, replace }),
  runStatus: (id: string) => invoke<RunStatus | null>("run_status", { id }),
  runStop: (id: string) => invoke<void>("run_stop", { id }),
  liveAttach: (id: string) => invoke<{ pid: number; package: string; target: string }>("live_attach", { id }),
  liveDetach: (id: string) => invoke<void>("live_detach", { id }),
  liveCounts: (id: string, classes: string[]) => invoke<LiveClassCount[]>("live_counts", { id, classes }),
  liveInstances: (id: string, cls: string) => invoke<LiveInstance[]>("live_instances", { id, class: cls }),
  liveObject: (id: string, object: string) => invoke<LiveObject>("live_object", { id, object }),
  liveSet: (id: string, object: string, field: string, value: string) =>
    invoke<string>("live_set", { id, object, field, value }),
  onRunProgress: (handler: (p: ImportProgress) => void): Promise<UnlistenFn> =>
    listen<ImportProgress>("run-progress", (e) => handler(e.payload)),
  toolInstall: (id: string) => invoke<ToolInfo>("tool_install", { id }),
  toolUninstall: (id: string) => invoke<void>("tool_uninstall", { id }),
  /** Which of these tools aren't available anywhere. */
  toolsMissing: (ids: string[]) => invoke<string[]>("tools_missing", { ids }),
  onToolProgress: (handler: (p: ToolProgress) => void): Promise<UnlistenFn> =>
    listen<ToolProgress>("tool-progress", (e) => handler(e.payload)),
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

/** Asks where to save a file of one type. */
export async function pickSaveFile(defaultName: string, label: string, extension: string): Promise<string | null> {
  return save({ defaultPath: defaultName, filters: [{ name: label, extensions: [extension] }] });
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
