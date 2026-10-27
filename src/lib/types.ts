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
  signerPath: string | null;
}

// Analyzer (src-tauri/src/model.rs, preview.rs)

export type Category =
  | "characters"
  | "core"
  | "map"
  | "items"
  | "assets"
  | "audio"
  | "ui"
  | "code"
  | "unsorted";

export interface Property {
  label: string;
  value: string;
}

export interface GameObject {
  id: string;
  category: Category;
  autoCategory: Category;
  displayName: string;
  kind: string;
  sourceFiles: string[];
  properties: Property[];
  confidence: number;
  reason: string;
  /** For characters: "Player", "Enemy", "NPC" or "Ally", when there is evidence. */
  role: string | null;
  /** Gameplay numbers found in the code, each with where it is set. */
  gameplay: GameplayValue[];
}

export interface GameplayValue {
  name: string;
  value: string;
  /** Path relative to unpacked/. */
  file: string;
  line: number;
  setIn: "field" | "constructor";
}

export interface EngineInfo {
  id: string;
  name: string;
  confidence: number;
  evidence: string[];
}

export type CapabilityStatus = "supported" | "partial" | "unsupported" | "unknown";

export interface Capability {
  feature: string;
  label: string;
  status: CapabilityStatus;
  note: string;
}

export interface GameProject {
  analyzerVersion: number;
  id: string;
  name: string;
  container: ContainerKind;
  engine: EngineInfo;
  workspacePath: string;
  analyzedAt: number;
  objects: GameObject[];
  capabilities: Capability[];
  notes: string[];
}

export type Preview =
  | { type: "image"; dataUrl: string; size: number }
  | { type: "audio"; dataUrl: string; size: number }
  | { type: "text"; text: string; truncated: boolean; size: number }
  | { type: "binary"; head: string; size: number }
  | { type: "tooLarge"; size: number };

// Editing and building (src-tauri/src/edits.rs, rebuild.rs)

export interface EditState {
  /** Paths (relative to unpacked/) that differ from the original. */
  edited: string[];
  /** What Undo would undo, if anything. */
  undo: string | null;
}

export interface BuildInfo {
  appName: string;
  package: string;
  versionName: string;
  versionCode: number;
  minSdk: string | null;
  permissions: string[];
  icon: string | null;
  problems: string[];
  keyExists: boolean;
  keyPath: string;
}

export interface BuildOptions {
  appName: string;
  package: string;
  versionName: string;
  versionCode: number;
  iconPath: string | null;
  removedPermissions: string[];
  outputPath: string;
}

export interface BuildResult {
  path: string;
  size: number;
  sha256: string;
  package: string;
  editedFiles: number;
}

// AI assistant (src-tauri/src/ai/)

export type AiProvider = "anthropic" | "openai" | "openrouter" | "gemini" | "groq";

export interface AiStatus {
  configured: boolean;
  provider: AiProvider | null;
  providerLabel: string | null;
  model: string | null;
  keyHint: string | null;
}

export interface ModelInfo {
  id: string;
  name: string;
}

export interface TokenUsage {
  input: number;
  output: number;
}

export interface UsageNote {
  where: string;
  how: string;
}

export interface TunableValue {
  id: string;
  name: string;
  description: string;
  current: string;
  type: "integer" | "decimal" | "boolean" | "text";
}

export interface Explanation {
  summary: string;
  explanation: string;
  usage: UsageNote[];
  values: TunableValue[];
  editTarget: string | null;
  tokens: TokenUsage;
  model: string;
  trimmed: string | null;
}

export interface Answer {
  answer: string;
  tokens: TokenUsage;
  model: string;
  trimmed: string | null;
}

export interface ValueChange {
  id: string;
  name: string;
  current: string;
  newValue: string;
}

export interface TextEdit {
  find: string;
  replace: string;
}

export interface Proposal {
  summary: string;
  notes: string;
  target: string;
  original: string;
  updated: string;
  edits: TextEdit[];
  tokens: TokenUsage;
  model: string;
  trimmed: string | null;
}

/** An image or sound found inside a file by its content (src-tauri/src/media.rs). */
export interface MediaItem {
  kind: "image" | "audio";
  format: string;
  mime: string;
  offset: number;
  len: number;
  width: number | null;
  height: number | null;
}

export interface ClassReference {
  /** The referring class, e.g. com.game.World. */
  class: string;
  method: string | null;
  line: string;
}

export interface References {
  items: ClassReference[];
  more: number;
  /** False for files that aren't Java/smali classes. */
  supported: boolean;
}
