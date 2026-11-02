import { useEffect, useRef, type ReactNode, type RefObject } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { TransformControls } from "three/examples/jsm/controls/TransformControls.js";
import { GLTFLoader } from "three/examples/jsm/loaders/GLTFLoader.js";
import { OBJLoader } from "three/examples/jsm/loaders/OBJLoader.js";
import { clone as cloneSkinned } from "three/examples/jsm/utils/SkeletonUtils.js";
import type { Entity, Part, Scene, Thing, Transform } from "../../lib/gx";
import { tidy, transformOf } from "../../lib/gxscene";

/** "select": pick things, no handles. */
export type GizmoMode = "select" | "translate" | "rotate" | "scale";

interface Props {
  scene: Scene;
  things: Record<string, Thing>;
  /** Bumped when entities are added, removed or change what they're made of. */
  structure: number;
  /** Selected entities; the last one is the main selection. */
  selected: string[];
  mode: GizmoMode;
  snap: boolean;
  showColliders: boolean;
  /** On screen: drawing pauses while it isn't. */
  active: boolean;
  /** A 3D scene seen straight from above, flat (the "Top View"). */
  top?: boolean;
  /** Where the pointer is in the scene is written here (no re-render). */
  cursor?: RefObject<HTMLElement | null>;
  /** Buttons floating over the selection. */
  toolbar?: ReactNode;
  /** An asset's bytes (path under the project's `assets/`). */
  readAsset: (asset: string) => Promise<ArrayBuffer>;
  /** `add`: Ctrl/Shift was held, so the selection grows or shrinks instead of being replaced. */
  onSelect: (id: string | null, add: boolean) => void;
  /** Entities were moved, turned or resized with the handles (on release). */
  onTransforms: (changes: { id: string; transform: Transform }[]) => void;
}

/** Editor-only markers (lights, cameras, placeholders) are drawn in this colour. */
const MARKER = 0x7a8cff;
const COLLIDER = 0x3ddc97;

/** A shape's geometry, `size` being its full extent in metres. */
function geometry(shape: Part & { type: "shape" | "collider" }, flat: boolean): THREE.BufferGeometry {
  const [x, y, z] = shape.size;
  switch (shape.shape) {
    case "sphere":
      return new THREE.SphereGeometry(Math.max(x, 0.001) / 2, 24, 16);
    case "capsule":
      return new THREE.CapsuleGeometry(Math.max(x, 0.001) / 2, Math.max(y - x, 0), 8, 16);
    case "cylinder":
      return new THREE.CylinderGeometry(x / 2, x / 2, Math.max(y, 0.001), 24);
    case "plane": {
      const g = new THREE.PlaneGeometry(Math.max(x, 0.001), Math.max(flat ? y : z, 0.001));
      if (!flat) g.rotateX(-Math.PI / 2);
      return g;
    }
    default:
      return new THREE.BoxGeometry(Math.max(x, 0.001), Math.max(y, 0.001), Math.max(z, 0.001));
  }
}

function marker(kind: "light" | "camera" | "placeholder", size = 0.3): THREE.Object3D {
  const material = new THREE.MeshBasicMaterial({ color: MARKER, wireframe: kind !== "light" });
  const geo =
    kind === "light"
      ? new THREE.SphereGeometry(size / 2, 12, 8)
      : kind === "camera"
        ? new THREE.ConeGeometry(size, size * 1.6, 4).rotateX(Math.PI / 2).translate(0, 0, -size * 0.8)
        : new THREE.BoxGeometry(size, size, size);
  const mesh = new THREE.Mesh(geo, material);
  mesh.userData.marker = true;
  return mesh;
}

/** The screen's pixel ratio, capped: a 2x screen draws 1.8x fewer pixels at 1.5 and looks the same. */
export function pixelRatio(): number {
  return Math.min(window.devicePixelRatio || 1, 1.5);
}

/** Images and models used by the scene, each read once. */
export class Assets {
  private textures = new Map<string, Promise<THREE.Texture>>();
  private models = new Map<string, Promise<THREE.Object3D>>();
  private urls: string[] = [];
  /** Told about each model once it's in place (the Player animates them). */
  onModel: ((model: THREE.Object3D) => void) | null = null;
  /** Told when an image or model has arrived, so a view that draws only on change draws again. */
  onLoad: (() => void) | null = null;
  constructor(private read: (asset: string) => Promise<ArrayBuffer>) {}

  private async url(asset: string, type = ""): Promise<string> {
    const url = URL.createObjectURL(new Blob([await this.read(asset)], { type }));
    this.urls.push(url);
    return url;
  }

  texture(asset: string): Promise<THREE.Texture> {
    let t = this.textures.get(asset);
    if (!t) {
      const type = asset.toLowerCase().endsWith(".svg") ? "image/svg+xml" : "";
      t = this.url(asset, type).then((url) => new THREE.TextureLoader().loadAsync(url)).then((tex) => {
        tex.colorSpace = THREE.SRGBColorSpace;
        // Small images are usually pixel art: keep their pixels sharp.
        const img = tex.image as { width?: number; height?: number } | undefined;
        if ((img?.width ?? 999) <= 256 && (img?.height ?? 999) <= 256) {
          tex.magFilter = THREE.NearestFilter;
          tex.minFilter = THREE.NearestFilter;
        }
        return tex;
      });
      t.then(() => this.onLoad?.()).catch(() => {});
      this.textures.set(asset, t);
    }
    return t;
  }

  model(asset: string): Promise<THREE.Object3D> {
    let m = this.models.get(asset);
    if (!m) {
      m = this.loadModel(asset);
      this.models.set(asset, m);
    }
    // Skinned models need their bones copied too; the clips come along in `animations`.
    return m.then((root) => cloneSkinned(root));
  }

  private async loadModel(asset: string): Promise<THREE.Object3D> {
    const lower = asset.toLowerCase();
    const dir = asset.slice(0, asset.lastIndexOf("/") + 1);
    if (lower.endsWith(".obj")) {
      return new OBJLoader().parse(new TextDecoder().decode(await this.read(asset)));
    }
    const bytes = await this.read(asset);
    // A .gltf (or a .glb with outside images) names its side files; they're read from beside it.
    const manager = new THREE.LoadingManager();
    const side = new Map<string, string>();
    const json = lower.endsWith(".gltf") ? JSON.parse(new TextDecoder().decode(bytes)) : glbJson(bytes);
    if (json) {
      const uris: string[] = [...(json.buffers ?? []), ...(json.images ?? [])].map((x: { uri?: string }) => x.uri).filter((u: string | undefined): u is string => !!u && !u.startsWith("data:"));
      for (const uri of uris) side.set(uri, await this.url(normalize(dir + decodeURIComponent(uri))).catch(() => ""));
    }
    manager.setURLModifier((url) => side.get(url) || side.get(url.split("/").pop() ?? "") || url);
    const gltf = await new GLTFLoader(manager).parseAsync(bytes, "");
    gltf.scene.animations = gltf.animations;
    return gltf.scene;
  }

  dispose() {
    this.textures.forEach((t) => t.then((tex) => tex.dispose()).catch(() => {}));
    this.urls.forEach((u) => URL.revokeObjectURL(u));
  }
}

/** What one part looks like in the editor. */
export function partObject(part: Part, flat: boolean, showColliders: boolean, assets?: Assets): THREE.Object3D | null {
  switch (part.type) {
    case "shape": {
      const mesh = new THREE.Mesh(
        geometry(part, flat),
        new THREE.MeshStandardMaterial({ color: new THREE.Color(part.color), roughness: 0.85, side: part.shape === "plane" ? THREE.DoubleSide : THREE.FrontSide }),
      );
      mesh.castShadow = true;
      mesh.receiveShadow = true;
      return mesh;
    }
    case "collider": {
      if (!showColliders) return null;
      // Walls and floors read from a mesh: every box's edges, in one drawing.
      const edges = part.boxes?.length ? boxEdges(part.boxes) : new THREE.EdgesGeometry(geometry(part, flat));
      const lines = new THREE.LineSegments(edges, new THREE.LineBasicMaterial({ color: part.trigger ? 0xf5b942 : COLLIDER }));
      // The engine finds colliders by this.
      lines.userData.collider = part;
      return lines;
    }
    case "light": {
      const color = new THREE.Color(part.color);
      const group = new THREE.Group();
      if (part.kind === "directional") {
        const light = new THREE.DirectionalLight(color, part.intensity * 2.2);
        light.castShadow = true;
        light.shadow.mapSize.set(2048, 2048);
        // Without a bias, surfaces shadow themselves in fine stripes.
        light.shadow.bias = -0.0005;
        light.shadow.normalBias = 0.02;
        const s = light.shadow.camera as THREE.OrthographicCamera;
        s.left = s.bottom = -25;
        s.right = s.top = 25;
        s.far = 80;
        // Shines along the entity's forward (-Z), wherever it's turned.
        const target = new THREE.Object3D();
        target.position.set(0, 0, -1);
        light.target = target;
        group.add(light, target);
      } else if (part.kind === "point") {
        group.add(new THREE.PointLight(color, part.intensity * 20, part.range ?? 0, 2));
      } else {
        const light = new THREE.SpotLight(color, part.intensity * 20, part.range ?? 0, Math.PI / 5, 0.3, 2);
        const target = new THREE.Object3D();
        target.position.set(0, 0, -1);
        light.target = target;
        group.add(light, target);
      }
      group.add(marker("light"));
      return group;
    }
    case "camera":
      return marker("camera");
    case "sprite": {
      const [w, h] = part.size ?? [1, 1];
      const material = new THREE.MeshBasicMaterial({ color: 0x9aa3c0, side: THREE.DoubleSide, transparent: true });
      const mesh = new THREE.Mesh(new THREE.PlaneGeometry(w, h), material);
      if (part.flipX) mesh.scale.x = -1;
      // Frames the Player shows in turn; the editor shows the first.
      if (part.frames && part.frames.length > 1) mesh.userData.frames = { assets: part.frames, fps: part.fps ?? 5 };
      assets
        ?.texture(part.asset)
        .then((tex) => {
          material.map = tex;
          material.color.set(0xffffff);
          material.alphaTest = 0.05;
          material.needsUpdate = true;
        })
        .catch(() => material.color.set(0xc8483c));
      return mesh;
    }
    case "model": {
      // A box shows where it is until the model has loaded.
      const holder = new THREE.Group();
      const box = marker("placeholder", 1);
      holder.add(box);
      assets
        ?.model(part.asset)
        .then((model) => {
          model.traverse((o) => {
            const m = o as THREE.Mesh;
            if (m.isMesh) {
              m.castShadow = true;
              m.receiveShadow = true;
            }
          });
          holder.remove(box);
          holder.add(model);
          assets.onModel?.(model);
          assets.onLoad?.();
        })
        .catch(() => ((box as THREE.Mesh).material as THREE.MeshBasicMaterial).color.set(0xc8483c));
      return holder;
    }
    case "text":
      return marker("placeholder", 0.5);
    case "tiles":
      return tilesObject(part, assets);
    default:
      return null;
  }
}

/** The edges of many boxes (`[min x, y, z, max x, y, z]`) as one line geometry. */
function boxEdges(boxes: number[][]): THREE.BufferGeometry {
  const out = new Float32Array(boxes.length * 24 * 3);
  let n = 0;
  for (const b of boxes) {
    const corner = (i: number) => [i & 1 ? b[3] : b[0], i & 2 ? b[4] : b[1], i & 4 ? b[5] : b[2]];
    // Corners that differ in one axis are joined.
    for (let i = 0; i < 8; i++) {
      for (const bit of [1, 2, 4]) {
        if (i & bit) continue;
        out.set(corner(i), n);
        out.set(corner(i | bit), n + 3);
        n += 6;
      }
    }
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.BufferAttribute(out, 3));
  return geo;
}

/** A tile grid: one mesh per sheet, its UVs set once the sheet's image is known. */
export function tilesObject(part: Part & { type: "tiles" }, assets?: Assets): THREE.Object3D {
  const group = new THREE.Group();
  const [cw, ch] = part.cell;
  part.sheets.forEach((sheet, index) => {
    const cells = part.cells.filter((c) => c[2] === index);
    if (!cells.length) return;
    const tw = sheet.tile[0] / 100;
    const th = sheet.tile[1] / 100;
    const positions = new Float32Array(cells.length * 12);
    const uvs = new Float32Array(cells.length * 8);
    const order: number[] = [];
    cells.forEach(([x, y], i) => {
      const cx = (x + 0.5) * cw;
      const cy = (y + 0.5) * ch;
      positions.set([cx - tw / 2, cy - th / 2, 0, cx + tw / 2, cy - th / 2, 0, cx + tw / 2, cy + th / 2, 0, cx - tw / 2, cy + th / 2, 0], i * 12);
      const v = i * 4;
      order.push(v, v + 1, v + 2, v, v + 2, v + 3);
    });
    const geo = new THREE.BufferGeometry();
    geo.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    geo.setAttribute("uv", new THREE.BufferAttribute(uvs, 2));
    geo.setIndex(order);
    const material = new THREE.MeshBasicMaterial({ color: 0x9aa3c0, transparent: true, alphaTest: 0.05, side: THREE.DoubleSide });
    group.add(new THREE.Mesh(geo, material));
    assets
      ?.texture(sheet.asset)
      .then((tex) => {
        const img = tex.image as { width: number; height: number };
        const [mx, my] = sheet.margin ?? [0, 0];
        const [sx, sy] = sheet.separation ?? [0, 0];
        cells.forEach(([, , , col, row, flags], i) => {
          const x0 = mx + col * (sheet.tile[0] + sx);
          const y0 = my + row * (sheet.tile[1] + sy);
          let u0 = x0 / img.width;
          let u1 = (x0 + sheet.tile[0]) / img.width;
          let vt = 1 - y0 / img.height;
          let vb = 1 - (y0 + sheet.tile[1]) / img.height;
          if (flags & 1) [u0, u1] = [u1, u0];
          if (flags & 2) [vt, vb] = [vb, vt];
          // Corners: bottom-left, bottom-right, top-right, top-left.
          const corners = [u0, vb, u1, vb, u1, vt, u0, vt];
          if (flags & 4) {
            // Transposed: top-right and bottom-left trade places.
            [corners[2], corners[3], corners[6], corners[7]] = [corners[6], corners[7], corners[2], corners[3]];
          }
          uvs.set(corners, i * 8);
        });
        geo.attributes.uv.needsUpdate = true;
        material.map = tex;
        material.color.set(0xffffff);
        material.needsUpdate = true;
      })
      .catch(() => material.color.set(0xc8483c));
  });
  return group;
}

/** `models/a/../textures/b.png` -> `models/textures/b.png`. */
function normalize(path: string): string {
  const out: string[] = [];
  for (const seg of path.split("/")) {
    if (seg === "..") out.pop();
    else if (seg && seg !== ".") out.push(seg);
  }
  return out.join("/");
}

/** The JSON chunk of a GLB file. */
function glbJson(bytes: ArrayBuffer): { buffers?: { uri?: string }[]; images?: { uri?: string }[] } | null {
  const view = new DataView(bytes);
  if (bytes.byteLength < 20 || view.getUint32(0, true) !== 0x46546c67) return null;
  const length = view.getUint32(12, true);
  try {
    return JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, 20, length)));
  } catch {
    return null;
  }
}

export function applyTransform(obj: THREE.Object3D, t: Transform) {
  obj.position.fromArray(t.position);
  obj.quaternion.fromArray(t.rotation);
  obj.scale.fromArray(t.scale);
}

export function dispose(root: THREE.Object3D) {
  root.traverse((o) => {
    const m = o as THREE.Mesh;
    m.geometry?.dispose();
    for (const mat of Array.isArray(m.material) ? m.material : m.material ? [m.material] : []) mat.dispose();
  });
}

/** Builds an entity: its own parts, its thing's parts and children, then its children. */
export function build(
  e: Entity,
  things: Record<string, Thing>,
  flat: boolean,
  showColliders: boolean,
  index: Map<string, THREE.Object3D>,
  owned: boolean,
  assets: Assets,
): THREE.Object3D {
  const group = new THREE.Group();
  group.name = e.name;
  applyTransform(group, transformOf(e));
  group.visible = e.active !== false;
  if (!owned) {
    group.userData.entityId = e.id;
    index.set(e.id, group);
  }
  const thing = e.thing ? things[e.thing] : undefined;
  for (const part of [...(thing?.parts ?? []), ...(e.parts ?? [])]) {
    const obj = partObject(part, flat, showColliders, assets);
    if (obj) group.add(obj);
  }
  if (e.thing && !thing) group.add(marker("placeholder", 0.8));
  for (const child of thing?.children ?? []) group.add(build(child, things, flat, showColliders, index, true, assets));
  for (const child of e.children ?? []) group.add(build(child, things, flat, showColliders, index, owned, assets));
  return group;
}

interface View {
  renderer: THREE.WebGLRenderer;
  world: THREE.Scene;
  camera: THREE.PerspectiveCamera | THREE.OrthographicCamera;
  orbit: OrbitControls;
  gizmo: TransformControls;
  content: THREE.Group;
  index: Map<string, THREE.Object3D>;
  outline: THREE.Box3Helper;
  /** What's selected, and the point the handles sit on when several are. */
  picked: THREE.Object3D[];
  pivot: THREE.Object3D;
  dragging: boolean;
  draw: () => void;
  assets: Assets;
  /** Metres per second when flying (Shift: four times). */
  flySpeed: number;
  /** Frames still to draw; the view only draws when something has changed. */
  dirty: number;
}

/**
 * The scene, drawn as the GameX engine will draw it: drag to orbit (3D),
 * right-drag to pan, scroll to zoom, click to select, handles to move,
 * turn and resize. F frames the selection.
 */
export default function SceneView(props: Props) {
  const { scene, things, structure, selected, mode, snap, showColliders, active, top = false, toolbar } = props;
  const host = useRef<HTMLDivElement>(null);
  const floating = useRef<HTMLDivElement>(null);
  const view = useRef<View | null>(null);
  const handlers = useRef(props);
  handlers.current = props;
  const flat = scene.dimensions === "2d";
  // Drawn flat with an orthographic camera: 2D scenes, and 3D ones from above.
  const ortho = flat || top;

  // The renderer, cameras and controls: once per scene.
  useEffect(() => {
    const el = host.current!;
    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(pixelRatio());
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFSoftShadowMap;
    el.appendChild(renderer.domElement);
    const world = new THREE.Scene();
    const camera = ortho ? new THREE.OrthographicCamera(-8, 8, 6, -6, top ? -2000 : -100, top ? 2000 : 100) : new THREE.PerspectiveCamera(55, 1, 0.05, 2000);
    if (flat) camera.position.set(0, 0, 50);
    if (top) {
      // Looking down, north (-Z) at the top of the view.
      camera.up.set(0, 0, -1);
      camera.position.set(0, 500, 0);
      camera.lookAt(0, 0, 0);
    }
    const orbit = new OrbitControls(camera, renderer.domElement);
    orbit.enableDamping = true;
    if (ortho) {
      orbit.enableRotate = false;
      orbit.screenSpacePanning = true;
      orbit.mouseButtons = { LEFT: THREE.MOUSE.PAN, MIDDLE: THREE.MOUSE.DOLLY, RIGHT: THREE.MOUSE.PAN };
    }
    const gizmo = new TransformControls(camera, renderer.domElement);
    world.add(gizmo.getHelper());
    const content = new THREE.Group();
    world.add(content);
    const outline = new THREE.Box3Helper(new THREE.Box3(), 0xffc94d);
    outline.visible = false;
    world.add(outline);
    const pivot = new THREE.Object3D();
    world.add(pivot);
    const grid = new THREE.GridHelper(60, 60, 0x6670a0, 0x3a4060);
    (grid.material as THREE.Material).transparent = true;
    (grid.material as THREE.Material).opacity = 0.45;
    // Just under the floor (or behind a 2D scene), so it never shows through.
    if (flat) {
      grid.rotateX(Math.PI / 2);
      grid.position.z = -0.6;
    } else {
      grid.position.y = -0.02;
    }
    world.add(grid);
    // Flying (3D): hold the right button to look around; WASD (or the arrows) move, Q/E down and up, Shift faster.
    // The camera goes through walls; letting go hands back to orbiting round a point ahead.
    const fly = { on: false, hover: false, keys: new Set<string>(), last: 0, yaw: 0, pitch: 0, x: 0, y: 0 };
    const FLY_KEYS = new Set(["w", "a", "s", "d", "q", "e", "arrowup", "arrowdown", "arrowleft", "arrowright", "pageup", "pagedown", "shift"]);
    /** Moves the camera for the held keys; true if it moved. */
    const flyMove = (now: number): boolean => {
      const dt = Math.min(0.1, fly.last ? (now - fly.last) / 1000 : 0);
      fly.last = now;
      if (ortho || dt === 0 || fly.keys.size === 0) return false;
      const k = (...names: string[]) => (names.some((n) => fly.keys.has(n)) ? 1 : 0);
      const held = fly.on ? 1 : 0;
      const forward = held * (k("w") - k("s")) + k("arrowup") - k("arrowdown");
      const side = held * (k("d") - k("a")) + k("arrowright") - k("arrowleft");
      const up = held * (k("e") - k("q")) + k("pageup") - k("pagedown");
      if (!forward && !side && !up) return false;
      const dir = camera.getWorldDirection(new THREE.Vector3());
      const right = new THREE.Vector3().crossVectors(dir, camera.up).normalize();
      const step = new THREE.Vector3().addScaledVector(dir, forward).addScaledVector(right, side).addScaledVector(camera.up, up);
      step.normalize().multiplyScalar(v.flySpeed * (fly.keys.has("shift") ? 4 : 1) * dt);
      camera.position.add(step);
      orbit.target.add(step);
      return true;
    };
    // Drawn only when something changed (the camera moved, an edit, a load), so an idle view costs nothing.
    const draw = () => {
      const flew = flyMove(performance.now());
      // While looking around the camera turns itself; orbiting would pull it back to its point.
      const orbited = !fly.on && orbit.update();
      if (!flew && !orbited && !fly.on && v.dirty <= 0) return;
      v.dirty = Math.max(0, v.dirty - 1);
      // One box around everything selected, following it as it moves.
      outline.visible = v.picked.length > 0;
      if (outline.visible) {
        outline.box.makeEmpty();
        for (const obj of v.picked) outline.box.expandByObject(obj);
      }
      // The floating buttons sit just above the selection's box on screen.
      const bar = floating.current;
      if (bar) {
        if (outline.visible && !outline.box.isEmpty() && !v.dragging) {
          const b = outline.box;
          const corners = [0, 1].flatMap((i) => [0, 1].flatMap((j) => [0, 1].map((k) => new THREE.Vector3(i ? b.max.x : b.min.x, j ? b.max.y : b.min.y, k ? b.max.z : b.min.z).project(camera))));
          const xs = corners.map((c) => c.x);
          const ys = corners.map((c) => c.y);
          const x = ((Math.min(...xs) + Math.max(...xs)) / 2 + 1) / 2;
          const y = (1 - Math.max(...ys)) / 2;
          bar.style.display = x > 0 && x < 1 && y > 0 && y < 1 ? "flex" : "none";
          bar.style.left = `${x * 100}%`;
          bar.style.top = `${y * 100}%`;
        } else bar.style.display = "none";
      }
      renderer.render(world, camera);
    };
    const v: View = { renderer, world, camera, orbit, gizmo, content, index: new Map(), outline, picked: [], pivot, dragging: false, draw, assets: new Assets((a) => handlers.current.readAsset(a)), flySpeed: 5, dirty: 2 };
    view.current = v;
    const wake = () => (v.dirty = Math.max(v.dirty, 2));
    v.assets.onLoad = wake;
    gizmo.addEventListener("change", wake);
    gizmo.addEventListener("objectChange", wake);
    for (const type of ["pointerdown", "pointermove", "pointerup", "wheel"]) renderer.domElement.addEventListener(type, wake);
    if (!ortho) {
      // The right button looks around (not pans); the middle one pans.
      orbit.mouseButtons = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.PAN, RIGHT: -1 as THREE.MOUSE };
    }
    const onFlyDown = (e: PointerEvent) => {
      if (ortho || e.button !== 2) return;
      const euler = new THREE.Euler().setFromQuaternion(camera.quaternion, "YXZ");
      Object.assign(fly, { on: true, yaw: euler.y, pitch: euler.x, x: e.clientX, y: e.clientY });
      orbit.enabled = false;
      renderer.domElement.setPointerCapture(e.pointerId);
    };
    const onFlyMove = (e: PointerEvent) => {
      if (!fly.on) return;
      fly.yaw -= (e.clientX - fly.x) * 0.004;
      fly.pitch = Math.max(-1.55, Math.min(1.55, fly.pitch - (e.clientY - fly.y) * 0.004));
      fly.x = e.clientX;
      fly.y = e.clientY;
      camera.quaternion.setFromEuler(new THREE.Euler(fly.pitch, fly.yaw, 0, "YXZ"));
    };
    const onFlyUp = (e: PointerEvent) => {
      if (!fly.on || e.button !== 2) return;
      fly.on = false;
      for (const k of ["w", "a", "s", "d", "q", "e"]) fly.keys.delete(k);
      // Orbit from here on round a point just ahead, so nothing jumps.
      const reach = Math.max(1, camera.position.distanceTo(orbit.target));
      orbit.target.copy(camera.position).addScaledVector(camera.getWorldDirection(new THREE.Vector3()), Math.min(reach, 10));
      orbit.enabled = !v.dragging;
    };
    const noMenu = (e: Event) => e.preventDefault();
    const onEnter = () => (fly.hover = true);
    const onLeave = () => (fly.hover = false);
    // Keys reach the view first: while flying (or arrows over the view) they move the camera, not the tools.
    const onKeyDown = (e: KeyboardEvent) => {
      if (handlers.current.active) wake();
      if (!handlers.current.active || ortho || (e.target as HTMLElement).closest?.("input, textarea, select")) return;
      const k = e.key.toLowerCase();
      if (!FLY_KEYS.has(k)) return;
      const arrow = k.startsWith("arrow") || k.startsWith("page");
      if (!(fly.on || (arrow && fly.hover))) return;
      if (k !== "shift") e.preventDefault();
      fly.keys.add(k);
    };
    const onKeyUp = (e: KeyboardEvent) => fly.keys.delete(e.key.toLowerCase());
    const onBlur = () => fly.keys.clear();
    renderer.domElement.addEventListener("pointerdown", onFlyDown);
    renderer.domElement.addEventListener("pointermove", onFlyMove);
    renderer.domElement.addEventListener("pointerup", onFlyUp);
    renderer.domElement.addEventListener("contextmenu", noMenu);
    renderer.domElement.addEventListener("pointerenter", onEnter);
    renderer.domElement.addEventListener("pointerleave", onLeave);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);

    // The handles also report "not dragging" when attached or detached, so
    // only the end of a drag that really started counts as an edit.
    // Several selected: the handles move a pivot, and each object follows it
    // from where it was when the drag began.
    let dragStarted = false;
    let starts: THREE.Matrix4[] = [];
    let pivotStart = new THREE.Matrix4();
    gizmo.addEventListener("dragging-changed", (event) => {
      v.dragging = Boolean(event.value);
      orbit.enabled = !v.dragging;
      if (v.dragging) {
        dragStarted = true;
        pivot.updateMatrixWorld();
        pivotStart = pivot.matrixWorld.clone();
        starts = v.picked.map((o) => {
          o.updateMatrixWorld();
          return o.matrixWorld.clone();
        });
        return;
      }
      if (!dragStarted) return;
      dragStarted = false;
      const changes = v.picked
        .filter((o) => o.userData.entityId)
        .map((o) => ({
          id: o.userData.entityId as string,
          transform: tidy({
            position: o.position.toArray() as Transform["position"],
            rotation: o.quaternion.toArray() as Transform["rotation"],
            scale: o.scale.toArray() as Transform["scale"],
          }),
        }));
      if (changes.length) handlers.current.onTransforms(changes);
    });
    gizmo.addEventListener("objectChange", () => {
      if (!v.dragging || gizmo.object !== pivot) return;
      pivot.updateMatrixWorld();
      const delta = pivot.matrixWorld.clone().multiply(pivotStart.clone().invert());
      v.picked.forEach((o, i) => {
        const world = delta.clone().multiply(starts[i]);
        const local = o.parent ? o.parent.matrixWorld.clone().invert().multiply(world) : world;
        local.decompose(o.position, o.quaternion, o.scale);
      });
    });

    // A click (not a drag) selects what's under the pointer.
    const ray = new THREE.Raycaster();
    let down: { x: number; y: number } | null = null;
    const onDown = (e: PointerEvent) => (down = { x: e.clientX, y: e.clientY });
    const onUp = (e: PointerEvent) => {
      if (!down || Math.hypot(e.clientX - down.x, e.clientY - down.y) > 4 || e.button !== 0) return;
      down = null;
      if (gizmo.axis) return;
      const rect = renderer.domElement.getBoundingClientRect();
      ray.setFromCamera(new THREE.Vector2(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1), camera);
      // Hidden objects (eye off, or inside a hidden parent) don't catch the click.
      const shown = (o: THREE.Object3D | null): boolean => !o || (o.visible && shown(o.parent));
      const hit = ray.intersectObject(content, true).find((h) => (h.object as THREE.Mesh).isMesh && shown(h.object));
      let obj: THREE.Object3D | null = hit?.object ?? null;
      while (obj && !obj.userData.entityId) obj = obj.parent;
      handlers.current.onSelect(obj?.userData.entityId ?? null, e.ctrlKey || e.shiftKey || e.metaKey);
    };
    // Where the pointer is, on the ground (3D) or the picture's plane (2D).
    const groundPlane = flat ? new THREE.Plane(new THREE.Vector3(0, 0, 1), 0) : new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
    const hitPoint = new THREE.Vector3();
    const onMove = (e: PointerEvent) => {
      const out = handlers.current.cursor?.current;
      if (!out) return;
      const rect = renderer.domElement.getBoundingClientRect();
      ray.setFromCamera(new THREE.Vector2(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1), camera);
      if (!ray.ray.intersectPlane(groundPlane, hitPoint)) return;
      out.textContent = flat ? `X: ${hitPoint.x.toFixed(2)}  Y: ${hitPoint.y.toFixed(2)}` : `X: ${hitPoint.x.toFixed(2)}  Z: ${hitPoint.z.toFixed(2)}`;
    };
    renderer.domElement.addEventListener("pointerdown", onDown);
    renderer.domElement.addEventListener("pointerup", onUp);
    renderer.domElement.addEventListener("pointermove", onMove);

    const resize = () => {
      const w = Math.max(1, el.clientWidth);
      const h = Math.max(1, el.clientHeight);
      renderer.setSize(w, h);
      if (camera instanceof THREE.PerspectiveCamera) {
        camera.aspect = w / h;
      } else {
        const half = (camera.top - camera.bottom) / 2;
        camera.left = (-half * w) / h;
        camera.right = (half * w) / h;
      }
      camera.updateProjectionMatrix();
      wake();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(el);
    resize();
    return () => {
      renderer.setAnimationLoop(null);
      for (const type of ["pointerdown", "pointermove", "pointerup", "wheel"]) renderer.domElement.removeEventListener(type, wake);
      observer.disconnect();
      renderer.domElement.removeEventListener("pointerdown", onDown);
      renderer.domElement.removeEventListener("pointerup", onUp);
      renderer.domElement.removeEventListener("pointermove", onMove);
      renderer.domElement.removeEventListener("pointerdown", onFlyDown);
      renderer.domElement.removeEventListener("pointermove", onFlyMove);
      renderer.domElement.removeEventListener("pointerup", onFlyUp);
      renderer.domElement.removeEventListener("contextmenu", noMenu);
      renderer.domElement.removeEventListener("pointerenter", onEnter);
      renderer.domElement.removeEventListener("pointerleave", onLeave);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
      gizmo.detach();
      gizmo.dispose();
      orbit.dispose();
      dispose(world);
      v.assets.dispose();
      renderer.dispose();
      el.removeChild(renderer.domElement);
      view.current = null;
    };
  }, [scene.id, flat, top]);

  // Anything React changed (the scene, the selection, the tools) is drawn next frame.
  useEffect(() => {
    if (view.current) view.current.dirty = Math.max(view.current.dirty, 2);
  });

  // Draws only while on screen.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.renderer.setAnimationLoop(active ? v.draw : null);
    return () => v.renderer.setAnimationLoop(null);
  }, [active, scene.id, flat, top]);

  // What's in the scene: rebuilt when its structure changes.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.gizmo.detach();
    for (const child of [...v.content.children]) {
      v.content.remove(child);
      dispose(child);
    }
    v.index.clear();
    v.world.background = new THREE.Color(scene.environment.background);
    v.content.add(new THREE.AmbientLight(new THREE.Color(scene.environment.ambient), 2.5));
    const lit = scene.entities.some((e) => [e, ...(e.children ?? [])].some((x) => x.parts?.some((p) => p.type === "light")));
    // A soft fill so sides facing away from the lights stay readable while
    // editing; a scene with no lights of its own gets a full one.
    v.content.add(new THREE.HemisphereLight(0xffffff, 0x404858, lit ? 0.8 : 1.5));
    for (const e of scene.entities) v.content.add(build(e, things, flat, showColliders, v.index, false, v.assets));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene.id, structure, things, showColliders, flat]);

  // When a scene opens: at the player's spawn, as in the game (3D); else the whole scene.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    if (!flat && !top && spawnView(v, scene, things)) return;
    frame(v, v.content, flat, top);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene.id, flat, top]);

  // Transforms changed in the inspector (or by undo) move objects without a rebuild.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    const visit = (list: Entity[]) => {
      for (const e of list) {
        const obj = v.index.get(e.id);
        if (obj && !(v.dragging && v.picked.includes(obj))) {
          applyTransform(obj, transformOf(e));
          obj.visible = e.active !== false;
        }
        visit(e.children ?? []);
      }
    };
    visit(scene.entities);
  }, [scene.entities]);

  // The selection: outline and handles.
  useEffect(() => {
    const v = view.current;
    if (!v) return;
    v.picked = selected.map((id) => v.index.get(id)).filter((o): o is THREE.Object3D => !!o);
    if (mode === "select") {
      v.gizmo.detach();
    } else if (v.picked.length === 1) {
      v.gizmo.attach(v.picked[0]);
    } else if (v.picked.length > 1) {
      // The handles sit at the middle of the selection, unturned.
      const middle = new THREE.Vector3();
      for (const o of v.picked) middle.add(o.getWorldPosition(new THREE.Vector3()));
      v.pivot.position.copy(middle.divideScalar(v.picked.length));
      v.pivot.quaternion.identity();
      v.pivot.scale.set(1, 1, 1);
      v.gizmo.attach(v.pivot);
    } else {
      v.gizmo.detach();
    }
    if (mode !== "select") v.gizmo.setMode(mode);
    v.gizmo.setTranslationSnap(snap ? 0.25 : null);
    v.gizmo.setRotationSnap(snap ? THREE.MathUtils.degToRad(15) : null);
    v.gizmo.setScaleSnap(snap ? 0.1 : null);
    // 2D: move in the plane, turn about the axis facing you. From above: the ground's axes, turning about up.
    if (top) {
      v.gizmo.showY = mode === "rotate";
      v.gizmo.showX = mode !== "rotate";
      v.gizmo.showZ = mode !== "rotate";
    } else {
      v.gizmo.showZ = !flat || mode === "rotate";
      v.gizmo.showX = !flat || mode !== "rotate";
      v.gizmo.showY = !flat || mode !== "rotate";
    }
  }, [selected.join("|"), mode, snap, structure, flat, top]);

  const onKey = (e: React.KeyboardEvent) => {
    const v = view.current;
    if (!v || e.key.toLowerCase() !== "f") return;
    if (v.picked.length === 0) return frame(v, v.content, flat, top);
    const box = new THREE.Box3();
    for (const o of v.picked) box.union(boundsOf(o));
    frameBox(v, box, flat, top);
  };

  return (
    <div
      ref={host}
      className="gx-canvas"
      tabIndex={0}
      onKeyDown={onKey}
      title={
        ortho
          ? "Drag or right-drag to move around, scroll to zoom, click to select. F frames the selection."
          : "Hold the right button to look around; with it held, WASD move, Q/E down and up, Shift faster. Arrows move too. Drag to orbit, middle-drag to pan, scroll to zoom, click to select. F frames the selection."
      }
    >
      {toolbar && (
        <div ref={floating} className="gx-floating" style={{ display: "none" }}>
          {toolbar}
        </div>
      )}
    </div>
  );
}

/**
 * Where most of a scene is: a few huge pieces (a light box hundreds of
 * metres wide, a far-off helper) would otherwise frame an empty middle.
 */
function coreBounds(target: THREE.Object3D): THREE.Box3 {
  const boxes: THREE.Box3[] = [];
  target.updateWorldMatrix(true, true);
  target.traverse((o) => {
    const m = o as THREE.Mesh;
    if (m.isMesh && !m.userData.marker) boxes.push(new THREE.Box3().setFromObject(m));
  });
  if (boxes.length < 8) return boundsOf(target);
  const centers = boxes.map((b) => b.getCenter(new THREE.Vector3()));
  const pick = (axis: "x" | "y" | "z", f: number) => {
    const list = centers.map((c) => c[axis]).sort((a, b) => a - b);
    return list[Math.floor(f * (list.length - 1))];
  };
  const lo = new THREE.Vector3(pick("x", 0.05), pick("y", 0.05), pick("z", 0.05));
  const hi = new THREE.Vector3(pick("x", 0.95), pick("y", 0.95), pick("z", 0.95));
  const core = new THREE.Box3(lo, hi).expandByScalar(Math.max(1, lo.distanceTo(hi) * 0.1));
  const limit = core.getSize(new THREE.Vector3()).length() * 1.5;
  const box = new THREE.Box3();
  for (const b of boxes) if (b.intersectsBox(core) && b.getSize(new THREE.Vector3()).length() <= limit) box.union(b);
  return box.isEmpty() ? core : box;
}

/** What `target` covers, editor markers left out. */
function boundsOf(target: THREE.Object3D): THREE.Box3 {
  const box = new THREE.Box3();
  target.updateWorldMatrix(true, true);
  target.traverse((o) => {
    const m = o as THREE.Mesh;
    if (m.isMesh && !m.userData.marker) box.expandByObject(m);
  });
  // Only markers (a light, a camera): frame where it is.
  if (box.isEmpty()) box.setFromCenterAndSize(target.getWorldPosition(new THREE.Vector3()), new THREE.Vector3(2, 2, 2));
  return box;
}

/**
 * The player the game would start with (as Play picks it): a thing with the
 * player role, else one named so; one switched on (with all its parents)
 * first, else one the game switches on itself.
 */
function playerOf(scene: Scene, things: Record<string, Thing>): string | null {
  const all: { e: Entity; on: boolean }[] = [];
  const visit = (list: Entity[], on: boolean) => {
    for (const e of list) {
      all.push({ e, on: on && e.active !== false });
      visit(e.children ?? [], on && e.active !== false);
    }
  };
  visit(scene.entities, true);
  const tests = [(e: Entity) => !!e.thing && things[e.thing]?.role === "player", (e: Entity) => /\bplayer\b/i.test(e.name.replace(/([a-z])([A-Z])/g, "$1 $2"))];
  // The role first, then the name: being switched on only decides between the same kind.
  for (const test of tests) {
    const found = all.find((x) => x.on && test(x.e)) ?? all.find((x) => test(x.e));
    if (found) return found.e.id;
  }
  return null;
}

/** Puts the camera where the player starts, at eye height, looking into the level. False if there's no player. */
function spawnView(v: View, scene: Scene, things: Record<string, Thing>): boolean {
  const id = playerOf(scene, things);
  const player = id ? v.index.get(id) : undefined;
  if (!player || !(v.camera instanceof THREE.PerspectiveCamera)) return false;
  player.updateWorldMatrix(true, false);
  const feet = player.getWorldPosition(new THREE.Vector3());
  const scale = player.getWorldScale(new THREE.Vector3());
  const eye = feet.clone().add(new THREE.Vector3(0, 1.6 * Math.min(Math.abs(scale.y), 3), 0));
  // Toward the middle of the level (where most things are), level with the eye.
  const spots: THREE.Vector3[] = [];
  v.index.forEach((o) => spots.push(o.getWorldPosition(new THREE.Vector3())));
  const mid = (axis: "x" | "z") => spots.map((p) => p[axis]).sort((a, b) => a - b)[Math.floor(spots.length / 2)] ?? 0;
  const look = new THREE.Vector3(mid("x"), eye.y, mid("z"));
  if (look.distanceTo(eye) < 0.5) look.set(eye.x, eye.y, eye.z - 1);
  v.camera.position.copy(eye);
  v.camera.near = 0.05;
  v.camera.far = 5000;
  v.camera.updateProjectionMatrix();
  v.camera.lookAt(look);
  v.orbit.target.copy(eye).addScaledVector(look.clone().sub(eye).normalize(), 3);
  v.flySpeed = 5;
  return true;
}

/** Points the camera at `target`'s bounds. */
function frame(v: View, target: THREE.Object3D, flat: boolean, top = false) {
  const box = target === v.content ? coreBounds(target) : boundsOf(target);
  // An empty scene: a comfortable view of the origin.
  if (target === v.content && box.getSize(new THREE.Vector3()).length() < 3.5) {
    box.setFromCenterAndSize(new THREE.Vector3(), new THREE.Vector3(10, 10, 10));
  }
  frameBox(v, box, flat, top);
}

/** Points the camera at a box. */
function frameBox(v: View, box: THREE.Box3, flat: boolean, top = false) {
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  const radius = Math.max(size.length() / 2, 0.5);
  if (v.camera instanceof THREE.OrthographicCamera) {
    const el = v.renderer.domElement;
    const aspect = el.clientWidth / Math.max(1, el.clientHeight);
    // From above, the ground's depth (Z) runs up the view.
    const tall = top ? size.z : size.y;
    const half = Math.max(tall / 2, size.x / 2 / aspect, 1) * 1.15;
    v.camera.top = half;
    v.camera.bottom = -half;
    v.camera.left = -half * aspect;
    v.camera.right = half * aspect;
    v.camera.zoom = 1;
    if (top) v.camera.position.set(center.x, center.y + 500, center.z);
    else v.camera.position.set(center.x, center.y, 50);
    v.camera.updateProjectionMatrix();
  } else if (!flat) {
    const dir = new THREE.Vector3(0.55, 0.6, 1).normalize();
    v.camera.position.copy(center).addScaledVector(dir, radius * 2.1);
    v.camera.near = radius / 200;
    v.camera.far = radius * 200;
    v.camera.updateProjectionMatrix();
  }
  v.orbit.target.copy(center);
  v.orbit.update();
}
