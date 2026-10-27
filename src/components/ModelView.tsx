import { Box, Download, Play, Replace } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { api, formatBytes, pickFile, pickSaveFile } from "../lib/api";
import {
  type AuxFiles,
  type LoadedModel,
  bounds,
  disposeModel,
  exportModel,
  fitInto,
  isTextVariant,
  loadModel,
  modelStats,
} from "../lib/modelio";
import { FORMAT_NAMES, MODEL_FORMATS, replacePlan } from "../lib/models";
import type { EditState } from "../lib/types";

/** A model with the side files it loaded from, freed together. */
interface Opened {
  model: LoadedModel;
  bytes: Uint8Array;
  urls: string[];
}

function close(opened: Opened) {
  disposeModel(opened.model.root);
  opened.urls.forEach((u) => URL.revokeObjectURL(u));
}

/**
 * Reads a model and its side files. `read` fetches one file's bytes;
 * `aux` are the side files' paths, `dir` the model's folder (both with `/`).
 */
async function open(
  format: string,
  read: (path: string) => Promise<ArrayBuffer>,
  path: string,
  auxPaths: string[],
): Promise<Opened> {
  const dir = path.slice(0, path.lastIndexOf("/") + 1);
  const bytes = await read(path);
  const aux: AuxFiles = new Map();
  const urls: string[] = [];
  for (const p of auxPaths) {
    const url = URL.createObjectURL(new Blob([await read(p)]));
    urls.push(url);
    aux.set((p.startsWith(dir) ? p.slice(dir.length) : p).toLowerCase(), url);
  }
  try {
    const model = await loadModel(format, bytes, aux);
    return { model, bytes: new Uint8Array(bytes), urls };
  } catch (e) {
    urls.forEach((u) => URL.revokeObjectURL(u));
    throw e;
  }
}

const slash = (p: string) => p.replace(/\\/g, "/");
const extension = (p: string) => (p.split(".").pop() ?? "").toLowerCase();
const fileName = (p: string) => slash(p).split("/").pop() ?? p;

interface Props {
  projectId: string;
  path: string;
  format: string;
  size: number;
  /** Whether this copy is rebuilt from, so it can be replaced. */
  editable: boolean;
  onEdited?: (state: EditState) => void;
}

/** A game's 3D model: turntable view, what's in it, replace and export. */
export default function ModelView({ projectId, path, format, size, editable, onEdited }: Props) {
  const [opened, setOpened] = useState<Opened | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [wireframe, setWireframe] = useState(false);
  const [clip, setClip] = useState(-1);
  const [replacement, setReplacement] = useState<{ source: string; opened: Opened } | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    let mine: Opened | null = null;
    setOpened(null);
    setError(null);
    (async () => {
      const aux = await api.modelAux(projectId, path);
      const result = await open(format, (p) => api.readBytes(projectId, p), path, aux);
      if (!alive) return close(result);
      mine = result;
      setOpened(result);
    })().catch((e) => alive && setError(String(e)));
    return () => {
      alive = false;
      if (mine) close(mine);
    };
  }, [projectId, path, format]);

  const pickReplacement = async () => {
    setNote(null);
    const source = await pickFile("3D model", MODEL_FORMATS);
    if (!source) return;
    try {
      const aux = (await api.localModelAux(source)).map(slash);
      const result = await open(extension(source), (p) => api.readLocalBytes(p), slash(source), aux);
      setReplacement({ source, opened: result });
    } catch (e) {
      setNote(`Couldn't read ${fileName(source)}: ${e}`);
    }
  };

  const exportGlb = async () => {
    if (!opened) return;
    setNote(null);
    const stem = fileName(path).replace(/\.[^.]+$/, "");
    const dest = await pickSaveFile(`${stem}.glb`, "glTF binary", "glb");
    if (!dest) return;
    try {
      const bytes = await exportModel(opened.model.root, opened.model.animations, "glb");
      await api.saveLocalBytes(dest, bytes);
      setNote(`Saved ${fileName(dest)}. Open it in Blender or another 3D tool, then use Replace to put it back.`);
    } catch (e) {
      setNote(`Couldn't export: ${e}`);
    }
  };

  if (error) return <div className="preview muted">Couldn't show this model: {error}</div>;
  if (!opened) return <div className="preview muted">Loading the 3D model ({formatBytes(size)})…</div>;

  const stats = modelStats(opened.model.root);
  return (
    <div className="preview model-view">
      <div className="row model-bar">
        <label className="small">
          <input type="checkbox" checked={wireframe} onChange={(e) => setWireframe(e.target.checked)} /> Wireframe
        </label>
        {opened.model.animations.length > 0 && (
          <label className="small row">
            <Play size={13} />
            <select value={clip} onChange={(e) => setClip(Number(e.target.value))}>
              <option value={-1}>No animation</option>
              {opened.model.animations.map((a, i) => (
                <option key={i} value={i}>
                  {a.name || `Animation ${i + 1}`}
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="status-spacer" />
        <button className="small" onClick={exportGlb} title="Save a copy as GLB, to edit in Blender or another 3D tool">
          <Download size={13} /> Export…
        </button>
        {editable && (
          <button className="small primary" onClick={pickReplacement}>
            <Replace size={13} /> Replace…
          </button>
        )}
      </div>
      <ModelCanvas model={opened.model} wireframe={wireframe} clip={clip} />
      <ModelFacts model={opened.model} format={format} />
      {note && <div className="notice small">{note}</div>}
      {replacement && (
        <ReplaceDialog
          projectId={projectId}
          path={path}
          format={format}
          original={opened}
          source={replacement.source}
          replacement={replacement.opened}
          onClose={(state) => {
            close(replacement.opened);
            setReplacement(null);
            if (state) onEdited?.(state);
          }}
        />
      )}
      {stats.meshes === 0 && <div className="notice small">This file has no meshes to show.</div>}
    </div>
  );
}

function ModelFacts({ model, format }: { model: LoadedModel; format: string }) {
  const s = modelStats(model.root);
  const n = (v: number) => v.toLocaleString();
  const dims = s.size.map((v) => (Math.abs(v) >= 100 ? v.toFixed(0) : v.toPrecision(3))).join(" × ");
  return (
    <div className="muted small model-facts">
      {FORMAT_NAMES[format] ?? format} · {n(s.vertices)} vertices · {n(s.triangles)} triangles · {s.meshes} mesh
      {s.meshes === 1 ? "" : "es"} · {s.materials} material{s.materials === 1 ? "" : "s"} · {s.textures} texture
      {s.textures === 1 ? "" : "s"}
      {s.skinned ? " · has bones" : ""}
      {model.animations.length ? ` · ${model.animations.length} animation${model.animations.length === 1 ? "" : "s"}` : ""} ·
      size {dims}
      {model.missing.length > 0 && (
        <div>
          Not found next to the model: {model.missing.slice(0, 6).join(", ")}
          {model.missing.length > 6 ? ` and ${model.missing.length - 6} more` : ""}. The game may load these another
          way; the model is shown without them.
        </div>
      )}
    </div>
  );
}

/** Orbit view of a model: drag to turn, scroll to zoom, right-drag to pan. */
function ModelCanvas({ model, wireframe, clip }: { model: LoadedModel; wireframe: boolean; clip: number }) {
  const host = useRef<HTMLDivElement>(null);
  const mixer = useRef<THREE.AnimationMixer | null>(null);

  useEffect(() => {
    const el = host.current!;
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(window.devicePixelRatio);
    el.appendChild(renderer.domElement);
    const scene = new THREE.Scene();
    scene.add(new THREE.HemisphereLight(0xffffff, 0x445566, 2));
    const sun = new THREE.DirectionalLight(0xffffff, 2);
    sun.position.set(3, 5, 4);
    scene.add(sun);
    const parent = model.root.parent;
    scene.add(model.root);

    const box = new THREE.Box3().setFromObject(model.root);
    const center = box.isEmpty() ? new THREE.Vector3() : box.getCenter(new THREE.Vector3());
    const radius = box.isEmpty() ? 1 : box.getSize(new THREE.Vector3()).length() || 1;
    const camera = new THREE.PerspectiveCamera(45, 1, radius / 1000, radius * 100);
    camera.position.copy(center).add(new THREE.Vector3(0.6, 0.45, 1).normalize().multiplyScalar(radius * 1.4));
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.target.copy(center);
    controls.enableDamping = true;
    const grid = new THREE.GridHelper(radius * 2, 20, 0x888888, 0x555555);
    grid.position.set(center.x, box.isEmpty() ? 0 : box.min.y, center.z);
    scene.add(grid);

    mixer.current = new THREE.AnimationMixer(model.root);
    const clock = new THREE.Clock();
    const resize = () => {
      const w = Math.max(1, el.clientWidth);
      const h = Math.max(1, el.clientHeight);
      renderer.setSize(w, h);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    };
    const observer = new ResizeObserver(resize);
    observer.observe(el);
    resize();
    renderer.setAnimationLoop(() => {
      mixer.current?.update(clock.getDelta());
      controls.update();
      renderer.render(scene, camera);
    });
    return () => {
      renderer.setAnimationLoop(null);
      observer.disconnect();
      controls.dispose();
      mixer.current?.stopAllAction();
      mixer.current = null;
      scene.remove(model.root);
      if (parent) parent.add(model.root);
      grid.geometry.dispose();
      renderer.dispose();
      el.removeChild(renderer.domElement);
    };
  }, [model]);

  useEffect(() => {
    model.root.traverse((o) => {
      const mesh = o as THREE.Mesh;
      if (!mesh.isMesh) return;
      for (const m of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
        if ("wireframe" in m) (m as THREE.MeshStandardMaterial).wireframe = wireframe;
      }
    });
  }, [model, wireframe]);

  useEffect(() => {
    const m = mixer.current;
    if (!m) return;
    m.stopAllAction();
    if (clip >= 0 && model.animations[clip]) m.clipAction(model.animations[clip]).play();
  }, [model, clip]);

  return <div ref={host} className="model-canvas" title="Drag to turn, scroll to zoom, right-drag to move" />;
}

function ReplaceDialog(props: {
  projectId: string;
  path: string;
  format: string;
  original: Opened;
  source: string;
  replacement: Opened;
  onClose: (state?: EditState) => void;
}) {
  const { projectId, path, format, original, source, replacement, onClose } = props;
  const from = extension(source);
  const [fit, setFit] = useState(from !== format);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const plan = replacePlan(from, format, fit);
  const s = modelStats(replacement.model.root);
  const moving = replacement.model.animations.length > 0 || s.skinned;

  const notes: string[] = [];
  if (plan.kind === "copy") notes.push(`Same format: the file is used exactly as it is.`);
  if (plan.kind === "convert") {
    if (from !== format) notes.push(`Converted from ${FORMAT_NAMES[from] ?? from} to ${FORMAT_NAMES[format] ?? format}.`);
    if (format === "stl" || format === "ply") notes.push("This format holds shape only: materials and textures aren't kept.");
    if (format === "obj") notes.push("OBJ keeps the shape; materials aren't written, so the game keeps its own material file.");
    if (moving && format !== "glb" && format !== "gltf") notes.push("Animations and bones can't be stored in this format and are dropped.");
  }
  if (replacement.model.missing.length) notes.push(`Not found next to your model: ${replacement.model.missing.slice(0, 4).join(", ")}.`);

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      if (plan.kind === "copy") {
        onClose(await api.replaceFile(projectId, path, source));
        return;
      }
      const root = fit ? fitInto(replacement.model.root, bounds(original.model.root)) : replacement.model.root;
      const bytes = await exportModel(root, replacement.model.animations, format, isTextVariant(format, original.bytes));
      const label = `Replace ${fileName(path)} (from ${fileName(source)}${fit ? ", fitted" : ""})`;
      onClose(await api.saveFileBytes(projectId, path, bytes, label));
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal model-dialog" role="dialog" aria-modal="true" aria-labelledby="model-replace-title">
        <h2 id="model-replace-title">
          <Box size={16} /> Replace {fileName(path)}
        </h2>
        <ModelCanvas model={replacement.model} wireframe={false} clip={-1} />
        <div className="muted small">
          {fileName(source)}: {s.vertices.toLocaleString()} vertices · {s.triangles.toLocaleString()} triangles ·{" "}
          {s.materials} materials · {s.textures} textures
          {replacement.model.animations.length ? ` · ${replacement.model.animations.length} animations` : ""}
        </div>
        <label className="small model-fit">
          <input type="checkbox" checked={fit} onChange={(e) => setFit(e.target.checked)} /> Fit to the original's size
          and place (same height, centered, standing on the same floor)
        </label>
        {plan.kind === "unsupported" ? (
          <div className="panel error small">{plan.reason}</div>
        ) : (
          <ul className="small model-notes">
            {notes.map((n) => (
              <li key={n}>{n}</li>
            ))}
            <li>Saved as an edit you can undo; Build APK puts it in the game.</li>
          </ul>
        )}
        {error && <div className="panel error small">{error}</div>}
        <div className="modal-actions">
          <button onClick={() => onClose()} disabled={busy}>
            Cancel
          </button>
          <button className="primary" onClick={apply} disabled={busy || plan.kind === "unsupported"}>
            {busy ? "Replacing…" : "Replace"}
          </button>
        </div>
      </div>
    </div>
  );
}
