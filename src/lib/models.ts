// 3D model formats and the math for fitting a replacement model where the
// original was. No three.js here, so it can be tested on its own.

/** Formats the 3D viewer reads. */
export const MODEL_FORMATS = ["obj", "gltf", "glb", "fbx", "dae", "stl", "ply", "3ds", "md2"];

/** Formats GameX can write, so a replacement can be converted to them. */
export const WRITABLE_FORMATS = new Set(["obj", "gltf", "glb", "stl", "ply"]);

export const FORMAT_NAMES: Record<string, string> = {
  obj: "OBJ",
  gltf: "glTF",
  glb: "glTF binary (GLB)",
  fbx: "FBX",
  dae: "Collada (DAE)",
  stl: "STL",
  ply: "PLY",
  "3ds": "3DS",
  md2: "Quake MD2",
};

export function isModelPath(path: string): boolean {
  const name = path.split("/").pop() ?? path;
  const dot = name.lastIndexOf(".");
  return dot > 0 && MODEL_FORMATS.includes(name.slice(dot + 1).toLowerCase());
}

/** How a replacement in `from` format becomes the game's `to` format. */
export type ReplacePlan =
  | { kind: "copy" }
  | { kind: "convert" }
  | { kind: "unsupported"; reason: string };

export function replacePlan(from: string, to: string, fit: boolean): ReplacePlan {
  if (from === to && !fit) return { kind: "copy" };
  if (WRITABLE_FORMATS.has(to)) return { kind: "convert" };
  return {
    kind: "unsupported",
    reason:
      from === to
        ? `GameX can't write ${FORMAT_NAMES[to] ?? to} files, so it can't resize this one. Untick fitting to copy it as it is.`
        : `GameX can't write ${FORMAT_NAMES[to] ?? to} files. Pick a .${to} file, or convert yours to .${to} in a 3D tool first.`,
  };
}

export interface Box {
  min: [number, number, number];
  max: [number, number, number];
}

/**
 * Scale and offset that put a model with bounds `from` where a model with
 * bounds `to` was: the same largest size, centered the same way, standing
 * on the same floor (lowest y).
 */
export function fitTransform(from: Box, to: Box): { scale: number; offset: [number, number, number] } {
  const size = (b: Box) => Math.max(b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]);
  const fromSize = size(from);
  const scale = fromSize > 0 && size(to) > 0 ? size(to) / fromSize : 1;
  const center = (b: Box, i: number) => (b.min[i] + b.max[i]) / 2;
  return {
    scale,
    offset: [
      center(to, 0) - center(from, 0) * scale,
      to.min[1] - from.min[1] * scale,
      center(to, 2) - center(from, 2) * scale,
    ],
  };
}

/**
 * Finds a side file (texture, material) a model asks for. Models often
 * name textures by an absolute path from the artist's computer, so the
 * file name alone is tried too.
 */
export function matchAuxFile(url: string, files: Map<string, string>): string | undefined {
  const clean = decodeURIComponent(url).replace(/\\/g, "/").replace(/^(\.\/)+/, "").toLowerCase();
  const exact = files.get(clean);
  if (exact) return exact;
  const name = clean.split("/").pop() ?? clean;
  for (const [key, value] of files) {
    if ((key.split("/").pop() ?? key) === name) return value;
  }
  return undefined;
}
