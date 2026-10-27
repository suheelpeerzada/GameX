// Mirrors src-tauri/src/model.rs, tools.rs and settings.rs.

export type ContainerKind = "apk" | "zip" | "folder";
export type StepStatus = "done" | "skipped" | "failed";

export interface UnpackStep {
  id: string;
  label: string;
  status: StepStatus;
  note: string | null;
  output: string | null;
}

export interface ImportRecord {
  id: string;
  name: string;
  container: ContainerKind;
  sourcePath: string;
  importedAt: number;
  originalSha256: string | null;
  originalBytes: number;
  unpackedFiles: number;
  steps: UnpackStep[];
  workspacePath: string;
}

export interface ImportProgress {
  stage: string;
  message: string;
}

export interface ToolStatus {
  id: string;
  found: boolean;
  path: string | null;
  source: string | null;
  note: string;
}

export interface Settings {
  apktoolPath: string | null;
  jadxPath: string | null;
}
