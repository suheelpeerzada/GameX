// The GameX format (src-tauri/src/gx/model.rs), as the window sees it,
// and the calls that read and save GameX projects.
import { invoke } from "@tauri-apps/api/core";

export const FORMAT = 1;

export type Vec3 = [number, number, number];
/** Quaternion [x, y, z, w]. */
export type Quat = [number, number, number, number];

export type Value =
  | { type: "number"; value: number }
  | { type: "integer"; value: number }
  | { type: "bool"; value: boolean }
  | { type: "text"; value: string }
  | { type: "vector"; value: number[] }
  | { type: "color"; value: string }
  | { type: "thing"; value: string }
  | { type: "asset"; value: string };

export type Property = Value & { name: string; unit?: string; description?: string };

export interface Source {
  id: string;
  name: string;
  engine: string;
  engineVersion?: string;
  importedFrom?: string;
}

export interface ProjectInfo {
  format: number;
  id: string;
  name: string;
  sources?: Source[];
  settings: { startScene?: string; pixelsPerUnit: number };
}

export interface Transform {
  position: Vec3;
  rotation: Quat;
  scale: Vec3;
}

export type ShapeKind = "box" | "sphere" | "capsule" | "cylinder" | "plane";
export type Body = "static" | "dynamic" | "kinematic";
export type LightKind = "directional" | "point" | "spot";

export type Part =
  | { type: "shape"; shape: ShapeKind; size: Vec3; color: string }
  | { type: "model"; asset: string }
  | { type: "sprite"; asset: string; frame?: string; size?: [number, number]; flipX?: boolean; frames?: string[]; fps?: number }
  | { type: "collider"; shape: ShapeKind; size: Vec3; body: Body; trigger?: boolean; /** Walls and floors read from a mesh: [min x, y, z, max x, y, z] in the entity's space. */ boxes?: [number, number, number, number, number, number][] }
  | { type: "light"; kind: LightKind; color: string; intensity: number; range?: number }
  | { type: "camera"; fov?: number; orthoSize?: number }
  | { type: "sound"; asset: string; volume: number; looped?: boolean; autoplay?: boolean }
  | { type: "text"; text: string; size: number; color: string }
  /** A tile grid: cells are [x, y, sheet, column, row, flags] (flags 1 flip x, 2 flip y, 4 transpose). */
  | { type: "tiles"; cell: [number, number]; sheets: TileSheet[]; cells: [number, number, number, number, number, number][] }
  | { type: "custom"; name: string; values?: Record<string, Value> };

export interface TileSheet {
  asset: string;
  /** One tile's size in pixels. */
  tile: [number, number];
  margin?: [number, number];
  separation?: [number, number];
  /** Tiles [column, row] that block movement. */
  solid?: [number, number][];
}

export interface BehaviorUse {
  behavior: string;
  params?: Record<string, Value>;
}

export interface Entity {
  id: string;
  name: string;
  thing?: string;
  /** Absent = at the origin, unrotated, unscaled. */
  transform?: Transform;
  /** Absent = active. */
  active?: boolean;
  tags?: string[];
  parts?: Part[];
  behaviors?: BehaviorUse[];
  overrides?: Record<string, Value>;
  children?: Entity[];
}

export interface Scene {
  format: number;
  id: string;
  name: string;
  dimensions: "2d" | "3d";
  environment: { background: string; ambient: string; gravity: Vec3 };
  entities: Entity[];
}

export type ThingKind = "character" | "item" | "tool" | "prop" | "other";

export interface Thing {
  format: number;
  id: string;
  name: string;
  kind: ThingKind;
  role?: string;
  description?: string;
  properties?: Property[];
  parts?: Part[];
  behaviors?: BehaviorUse[];
  children?: Entity[];
  /** "no-move", "no-attack", "no-collide" switch those off for every instance. */
  tags?: string[];
}

export type Arg = { property: string } | Value;

export interface Block {
  block: string;
  args?: Record<string, Arg>;
}

export interface Rule {
  state?: string;
  when: Block;
  if?: Block[];
  then: Block[];
}

export interface Behavior {
  format: number;
  id: string;
  name: string;
  description?: string;
  params?: Property[];
  states?: string[];
  initialState?: string;
  rules: Rule[];
}

export interface Item {
  id: string;
  name: string;
  file: string;
  kind?: string;
}

/** Where a translated element came from (src-tauri/src/gx/model.rs Origin). */
export interface Origin {
  file: string;
  object?: string;
  field?: string;
  fidelity: "exact" | "approximated" | "missing";
  note?: string;
}

export interface ProjectIndex {
  path: string;
  info: ProjectInfo;
  scenes: Item[];
  things: Item[];
  behaviors: Item[];
  scripts: string[];
  assets: string[];
  problems: { file: string; message: string }[];
  undo?: string;
  redo?: string;
}

/** What a translation did (src-tauri/src/gx/godot.rs Report). */
export interface TranslationReport {
  source: string;
  /** "Godot 4", "Unity 2021.3.11f1". */
  engine: string;
  godotVersion: number;
  scenes: number;
  things: number;
  entities: number;
  assets: number;
  scripts: number;
  untranslated: Record<string, number>;
  problems: string[];
}

/** What sending the layer back into the game did (src-tauri/src/gx/export.rs). */
export interface SentToGame {
  written: string[];
  unchanged: number;
  /** [asset, why] */
  notWritable: [string, string][];
  /** Objects moved, turned, resized or hidden, written into the game's scene files (Godot projects). */
  objects: number;
}

export const gx = {
  openProject: (path: string) => invoke<ProjectIndex>("gx_open_project", { path }),
  /** The index and every thing, in one call. */
  openWithThings: (path: string) => invoke<{ index: ProjectIndex; things: Thing[] }>("gx_open_with_things", { path }),
  loadScene: (path: string, id: string) => invoke<Scene>("gx_load_scene", { path, id }),
  loadBehavior: (path: string, id: string) => invoke<Behavior>("gx_load_behavior", { path, id }),
  /** The save calls return what Undo would now take back. */
  saveScene: (path: string, scene: Scene, summary: string) => invoke<string | null>("gx_save_scene", { path, scene, summary }),
  saveThing: (path: string, thing: Thing, summary: string) => invoke<string | null>("gx_save_thing", { path, thing, summary }),
  saveBehavior: (path: string, behavior: Behavior, summary: string) =>
    invoke<string | null>("gx_save_behavior", { path, behavior, summary }),
  /** The layer made for an imported game, if any. */
  /** `outdated`: made by an older reader; reading the game again places things better. */
  layerForImport: (id: string) => invoke<{ path: string; outdated: boolean } | null>("gx_layer_for_import", { id }),
  /** Keeps an outdated layer (stops asking). */
  keepLayer: (id: string) => invoke<void>("gx_keep_layer", { id }),
  /** Combined Workspace: translates another imported game into this layer. */
  addGame: (path: string, id: string) =>
    invoke<{ merged: { source: string; scenes: number; things: number; assets: number; renamed: [string, string][] }; report: TranslationReport }>("gx_add_game", { path, id }),
  /** Reads how an imported game builds its levels and makes its layer. */
  makeLayer: (id: string) => invoke<{ path: string; report: TranslationReport }>("gx_make_layer", { id }),
  /** Output A: the layer's changed assets into the imported game's edits, for the usual Build. */
  sendToGame: (id: string) => invoke<SentToGame>("gx_send_to_game", { id }),
  /** Replaces a layer asset with a picked file; returns what Undo would take back. */
  replaceAsset: (path: string, asset: string, file: string) => invoke<string | null>("gx_replace_asset", { path, asset, file }),
  readAsset: (path: string, asset: string) => invoke<ArrayBuffer>("gx_read_asset", { path, asset }),
  deleteScene: (path: string, id: string, summary: string) => invoke<string | null>("gx_delete_scene", { path, id, summary }),
  saveInfo: (path: string, info: ProjectInfo, summary: string) => invoke<string | null>("gx_save_info", { path, info, summary }),
  /** [what was undone, what Undo would take back next]. */
  undo: (path: string) => invoke<[string | null, string | null]>("gx_undo", { path }),
  /** Where each translated element came from, and how faithful it is (see Origin). */
  origins: (path: string) => invoke<Record<string, Origin>>("gx_origins", { path }),
  /** [what was redone, what Redo would bring back next]. */
  redo: (path: string) => invoke<[string | null, string | null]>("gx_redo", { path }),
};
