// Reading and writing 3D models with three.js: every format the viewer
// shows, and the formats a replacement can be converted to.
import * as THREE from "three";
import { ColladaLoader } from "three/examples/jsm/loaders/ColladaLoader.js";
import { FBXLoader } from "three/examples/jsm/loaders/FBXLoader.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { MD2Loader } from "three/examples/jsm/loaders/MD2Loader.js";
import { MTLLoader } from "three/examples/jsm/loaders/MTLLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { PLYLoader } from "three/examples/jsm/loaders/PLYLoader.js";
import { STLLoader } from "three/examples/jsm/loaders/STLLoader.js";
import { TDSLoader } from "three/examples/jsm/loaders/TDSLoader.js";
import { GLTFExporter } from "three/examples/jsm/exporters/GLTFExporter.js";
import { OBJExporter } from "three/examples/jsm/exporters/OBJExporter.js";
import { PLYExporter } from "three/examples/jsm/exporters/PLYExporter.js";
import { STLExporter } from "three/examples/jsm/exporters/STLExporter.js";
import { type Box, fitTransform, matchAuxFile } from "./models.ts";

export interface LoadedModel {
  root: THREE.Object3D;
  animations: THREE.AnimationClip[];
  /** Side files the model asked for that weren't found. */
  missing: string[];
}

export interface ModelStats {
  meshes: number;
  vertices: number;
  triangles: number;
  materials: number;
  textures: number;
  skinned: boolean;
  size: [number, number, number];
}

/** Side files (lowercased path relative to the model's folder -> URL). */
export type AuxFiles = Map<string, string>;

const text = (bytes: ArrayBuffer) => new TextDecoder().decode(bytes);

function geometryMesh(geometry: THREE.BufferGeometry): THREE.Mesh {
  if (!geometry.getAttribute("normal")) geometry.computeVertexNormals();
  const material = new THREE.MeshStandardMaterial({
    color: 0xc8c8c8,
    vertexColors: !!geometry.getAttribute("color"),
    flatShading: !geometry.index,
  });
  return new THREE.Mesh(geometry, material);
}

/** Loads a model from its bytes. Textures load in the background. */
export async function loadModel(format: string, bytes: ArrayBuffer, aux: AuxFiles): Promise<LoadedModel> {
  const missing = new Set<string>();
  const manager = new THREE.LoadingManager();
  manager.setURLModifier((url) => {
    if (url.startsWith("data:") || url.startsWith("blob:")) return url;
    const found = matchAuxFile(url, aux);
    if (!found) missing.add(url.replace(/\\/g, "/").split("/").pop() ?? url);
    return found ?? url;
  });
  manager.onError = (url) => {
    if (!url.startsWith("blob:") && !url.startsWith("data:")) missing.add(url.split("/").pop() ?? url);
  };

  let root: THREE.Object3D;
  let animations: THREE.AnimationClip[] = [];
  switch (format) {
    case "obj": {
      const source = text(bytes);
      const loader = new OBJLoader(manager);
      const mtlName = /^mtllib\s+(.+?)\s*$/m.exec(source)?.[1];
      if (mtlName) {
        const url = matchAuxFile(mtlName, aux);
        if (url) {
          const materials = new MTLLoader(manager).parse(await (await fetch(url)).text(), "");
          materials.preload();
          loader.setMaterials(materials);
        } else missing.add(mtlName);
      }
      root = loader.parse(source);
      break;
    }
    case "gltf":
    case "glb": {
      const gltf = await new GLTFLoader(manager).parseAsync(format === "gltf" ? text(bytes) : bytes, "");
      root = gltf.scene;
      animations = gltf.animations;
      break;
    }
    case "fbx":
      root = new FBXLoader(manager).parse(bytes, "");
      animations = root.animations;
      break;
    case "dae": {
      const collada = new ColladaLoader(manager).parse(text(bytes), "");
      if (!collada) throw new Error("This Collada file couldn't be read.");
      root = collada.scene;
      animations = collada.scene.animations ?? [];
      break;
    }
    case "stl":
      root = geometryMesh(new STLLoader().parse(bytes));
      break;
    case "ply":
      root = geometryMesh(new PLYLoader().parse(bytes));
      break;
    case "3ds":
      root = new TDSLoader(manager).parse(bytes, "");
      break;
    case "md2": {
      const geometry = new MD2Loader().parse(bytes);
      root = geometryMesh(geometry);
      animations = (geometry as unknown as { animations?: THREE.AnimationClip[] }).animations ?? [];
      break;
    }
    default:
      throw new Error(`GameX can't read .${format} models.`);
  }
  root.updateMatrixWorld(true);
  return { root, animations, missing: [...missing] };
}

export function bounds(root: THREE.Object3D): Box {
  const box = new THREE.Box3().setFromObject(root);
  if (box.isEmpty()) return { min: [0, 0, 0], max: [0, 0, 0] };
  return { min: box.min.toArray(), max: box.max.toArray() };
}

export function modelStats(root: THREE.Object3D): ModelStats {
  const materials = new Set<THREE.Material>();
  const textures = new Set<THREE.Texture>();
  let meshes = 0;
  let vertices = 0;
  let triangles = 0;
  let skinned = false;
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh) return;
    meshes++;
    skinned ||= (o as THREE.SkinnedMesh).isSkinnedMesh === true;
    const position = mesh.geometry.getAttribute("position");
    vertices += position?.count ?? 0;
    triangles += Math.floor((mesh.geometry.index?.count ?? position?.count ?? 0) / 3);
    for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      materials.add(m);
      for (const value of Object.values(m)) if ((value as THREE.Texture)?.isTexture) textures.add(value as THREE.Texture);
    }
  });
  const b = bounds(root);
  return {
    meshes,
    vertices,
    triangles,
    materials: materials.size,
    textures: textures.size,
    skinned,
    size: [b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]],
  };
}

/** Moves and scales `model` so it stands where a model with bounds `target` stood. */
export function fitInto(model: THREE.Object3D, target: Box): THREE.Object3D {
  const { scale, offset } = fitTransform(bounds(model), target);
  const holder = new THREE.Group();
  holder.add(model);
  holder.scale.setScalar(scale);
  holder.position.set(...offset);
  holder.updateMatrixWorld(true);
  return holder;
}

/** Whether an STL or PLY file is the text kind (so a replacement matches it). */
export function isTextVariant(format: string, original: Uint8Array): boolean {
  const head = new TextDecoder().decode(original.slice(0, 1024));
  if (format === "stl") return head.startsWith("solid") && head.includes("facet");
  if (format === "ply") return head.includes("format ascii");
  return false;
}

/** Writes `root` as a `format` file. `textVariant` picks ASCII STL/PLY. */
export async function exportModel(
  root: THREE.Object3D,
  animations: THREE.AnimationClip[],
  format: string,
  textVariant = false,
): Promise<Uint8Array> {
  root.updateMatrixWorld(true);
  const encode = (s: string) => new TextEncoder().encode(s);
  switch (format) {
    case "obj":
      return encode(new OBJExporter().parse(root));
    case "stl": {
      const out = new STLExporter().parse(root, { binary: !textVariant });
      return typeof out === "string" ? encode(out) : new Uint8Array(out.buffer, out.byteOffset, out.byteLength);
    }
    case "ply": {
      // No callback: with one, the exporter waits for an animation frame.
      const out = new PLYExporter().parse(root, undefined as never, { binary: !textVariant });
      if (out === null) throw new Error("This model has nothing to write as PLY.");
      return typeof out === "string" ? encode(out) : new Uint8Array(out);
    }
    case "gltf":
    case "glb": {
      const out = await new GLTFExporter().parseAsync(root, { binary: format === "glb", animations });
      return out instanceof ArrayBuffer ? new Uint8Array(out) : encode(JSON.stringify(out));
    }
    default:
      throw new Error(`GameX can't write .${format} models.`);
  }
}

/** Frees a model's GPU memory. */
export function disposeModel(root: THREE.Object3D) {
  root.traverse((o) => {
    const mesh = o as THREE.Mesh;
    if (!mesh.isMesh) return;
    mesh.geometry.dispose();
    for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      for (const value of Object.values(m)) if ((value as THREE.Texture)?.isTexture) (value as THREE.Texture).dispose();
      m.dispose();
    }
  });
}
