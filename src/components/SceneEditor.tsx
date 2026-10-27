import { Maximize, Box } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { api } from "../lib/api";
import {
  IDENTITY,
  apply,
  extResourceId,
  formatNumber,
  formatVector2,
  fromParts,
  invert,
  multiply,
  parseNumber,
  parseVector2,
  type Matrix,
  type Vec2,
} from "../lib/godot";
import type { EditState, Scene, SceneNode, SceneProperty } from "../lib/types";

interface Props {
  projectId: string;
  /** The .tscn file, relative to unpacked/. */
  path: string;
  editable: boolean;
  revision: number;
  onEdited: (state: EditState) => void;
}

interface Texture {
  src: string;
  w: number;
  h: number;
}

/** How a node is drawn: its transform, and a rectangle (in its own space) if it has a size. */
interface Placed {
  node: SceneNode;
  props: Record<string, string>;
  global: Matrix;
  parentGlobal: Matrix;
  local: Vec2;
  control: boolean;
  rect: { x: number; y: number; w: number; h: number } | null;
  texture: Texture | null;
}

type ViewBox = { x: number; y: number; w: number; h: number };

const CONTROL_KEYS = ["offset_left", "offset_top", "offset_right", "offset_bottom"];
const NODE2D_DEFAULTS: [string, string][] = [
  ["position", "Vector2(0, 0)"],
  ["rotation", "0"],
  ["scale", "Vector2(1, 1)"],
  ["visible", "true"],
];

/**
 * A visual editor for Godot text scenes: the node tree, a canvas showing 2D
 * nodes and sprites where they are, dragging to move, and a property form.
 * Every change is saved to the .tscn as an edit that Undo reverts.
 */
export default function SceneEditor({ projectId, path, editable, revision, onEdited }: Props) {
  const [scene, setScene] = useState<Scene | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(".");
  const [textures, setTextures] = useState<Record<string, Texture>>({});
  const [view, setView] = useState<ViewBox | null>(null);
  // While dragging, the node's new local position (not saved yet).
  const [dragged, setDragged] = useState<{ node: string; pos: Vec2 } | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const gesture = useRef<
    | { kind: "node"; node: string; startWorld: Vec2; startGlobal: Vec2; parentInverse: Matrix; local: Vec2; moved: boolean }
    | { kind: "pan"; startScreen: Vec2; startView: ViewBox }
    | null
  >(null);

  useEffect(() => {
    let current = true;
    api
      .sceneLoad(projectId, path)
      .then((s) => {
        if (!current) return;
        setScene(s);
        setDragged(null);
        setError(null);
      })
      .catch((e) => current && setError(String(e)));
    return () => {
      current = false;
    };
  }, [projectId, path, revision]);

  // Load each texture the scene uses, with its pixel size.
  useEffect(() => {
    if (!scene) return;
    for (const res of scene.resources) {
      if (!res.file || textures[res.id] || !/texture|image/i.test(res.kind + res.path)) continue;
      api
        .readPreview(projectId, res.file)
        .then((p) => {
          if (p.type !== "image") return;
          const img = new Image();
          img.onload = () =>
            setTextures((t) => ({ ...t, [res.id]: { src: p.dataUrl, w: img.naturalWidth, h: img.naturalHeight } }));
          img.src = p.dataUrl;
        })
        .catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scene, projectId]);

  const placed = useMemo(() => {
    const out: Placed[] = [];
    if (!scene) return out;
    const byPath = new Map<string, Placed>();
    for (const node of scene.nodes) {
      const props = Object.fromEntries(node.properties.map((p) => [p.key, p.value]));
      const parentGlobal = (node.parent ? byPath.get(node.parent)?.global : undefined) ?? IDENTITY;
      const control = CONTROL_KEYS.some((k) => k in props);
      const stored: Vec2 = control
        ? { x: parseNumber(props.offset_left) ?? 0, y: parseNumber(props.offset_top) ?? 0 }
        : (parseVector2(props.position) ?? { x: 0, y: 0 });
      const local = dragged?.node === node.path ? dragged.pos : stored;
      const rotation =
        parseNumber(props.rotation) ?? ((parseNumber(props.rotation_degrees) ?? 0) * Math.PI) / 180;
      const scale = parseVector2(props.scale) ?? { x: 1, y: 1 };
      const global = multiply(parentGlobal, fromParts(local, rotation, scale));
      const textureId = extResourceId(props.texture);
      const texture = textureId ? (textures[textureId] ?? null) : null;

      let rect: Placed["rect"] = null;
      if (control) {
        const right = parseNumber(props.offset_right);
        const bottom = parseNumber(props.offset_bottom);
        const size = parseVector2(props.size);
        const w = right !== null ? right - stored.x : (size?.x ?? texture?.w ?? 0);
        const h = bottom !== null ? bottom - stored.y : (size?.y ?? texture?.h ?? 0);
        if (w > 0 && h > 0) rect = { x: 0, y: 0, w, h };
      } else if (texture) {
        const offset = parseVector2(props.offset) ?? { x: 0, y: 0 };
        const centered = props.centered !== "false";
        rect = {
          x: offset.x - (centered ? texture.w / 2 : 0),
          y: offset.y - (centered ? texture.h / 2 : 0),
          w: texture.w,
          h: texture.h,
        };
      }
      const p: Placed = { node, props, global, parentGlobal, local, control, rect, texture };
      byPath.set(node.path, p);
      out.push(p);
    }
    return out;
  }, [scene, textures, dragged]);

  // Fit the view to the scene's content the first time it's shown.
  const fit = () => {
    const points: Vec2[] = [];
    for (const p of placed) {
      if (p.rect) {
        const { x, y, w, h } = p.rect;
        for (const c of [{ x, y }, { x: x + w, y }, { x, y: y + h }, { x: x + w, y: y + h }]) points.push(apply(p.global, c));
      } else {
        points.push(apply(p.global, { x: 0, y: 0 }));
      }
    }
    if (points.length === 0) return;
    const xs = points.map((p) => p.x);
    const ys = points.map((p) => p.y);
    const minX = Math.min(...xs);
    const minY = Math.min(...ys);
    const w = Math.max(Math.max(...xs) - minX, 200);
    const h = Math.max(Math.max(...ys) - minY, 150);
    setView({ x: minX - w * 0.1, y: minY - h * 0.1, w: w * 1.2, h: h * 1.2 });
  };
  useEffect(() => {
    if (!view && placed.length > 0) fit();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [placed]);

  const toWorld = (clientX: number, clientY: number): Vec2 => {
    const svg = svgRef.current;
    const ctm = svg?.getScreenCTM();
    if (!svg || !ctm) return { x: 0, y: 0 };
    const pt = new DOMPoint(clientX, clientY).matrixTransform(ctm.inverse());
    return { x: pt.x, y: pt.y };
  };

  const save = async (node: string, changes: [string, string][]) => {
    try {
      onEdited(await api.sceneSet(projectId, path, node, changes));
    } catch (e) {
      setError(String(e));
      setDragged(null);
    }
  };

  const startNodeDrag = (e: ReactPointerEvent, p: Placed) => {
    e.stopPropagation();
    setSelected(p.node.path);
    if (!editable) return;
    svgRef.current?.setPointerCapture(e.pointerId);
    gesture.current = {
      kind: "node",
      node: p.node.path,
      startWorld: toWorld(e.clientX, e.clientY),
      startGlobal: apply(p.global, { x: 0, y: 0 }),
      parentInverse: invert(p.parentGlobal),
      local: p.local,
      moved: false,
    };
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    if (!view) return;
    svgRef.current?.setPointerCapture(e.pointerId);
    gesture.current = { kind: "pan", startScreen: { x: e.clientX, y: e.clientY }, startView: view };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const g = gesture.current;
    if (!g) return;
    if (g.kind === "node") {
      const world = toWorld(e.clientX, e.clientY);
      const global = { x: g.startGlobal.x + world.x - g.startWorld.x, y: g.startGlobal.y + world.y - g.startWorld.y };
      // Positions are stored relative to the parent.
      const local = apply(g.parentInverse, global);
      g.local = local;
      g.moved = true;
      setDragged({ node: g.node, pos: local });
    } else {
      const svg = svgRef.current;
      if (!svg) return;
      const scale = g.startView.w / svg.clientWidth;
      setView({
        ...g.startView,
        x: g.startView.x - (e.clientX - g.startScreen.x) * scale,
        y: g.startView.y - (e.clientY - g.startScreen.y) * scale,
      });
    }
  };

  const onPointerUp = () => {
    const g = gesture.current;
    gesture.current = null;
    if (g?.kind !== "node" || !g.moved) return;
    const p = placed.find((x) => x.node.path === g.node);
    if (!p) return;
    if (p.control) {
      // Controls keep their rectangle: move all four offsets together.
      const dx = g.local.x - (parseNumber(p.props.offset_left) ?? 0);
      const dy = g.local.y - (parseNumber(p.props.offset_top) ?? 0);
      const changes: [string, string][] = [];
      for (const [key, d] of [["offset_left", dx], ["offset_right", dx], ["offset_top", dy], ["offset_bottom", dy]] as const) {
        const v = parseNumber(p.props[key]);
        if (v !== null || key === "offset_left" || key === "offset_top") changes.push([key, formatNumber((v ?? 0) + d)]);
      }
      save(g.node, changes);
    } else {
      save(g.node, [["position", formatVector2(g.local)]]);
    }
  };

  const onWheel = (e: React.WheelEvent) => {
    if (!view) return;
    const world = toWorld(e.clientX, e.clientY);
    const factor = e.deltaY > 0 ? 1.15 : 1 / 1.15;
    setView({
      x: world.x - (world.x - view.x) * factor,
      y: world.y - (world.y - view.y) * factor,
      w: view.w * factor,
      h: view.h * factor,
    });
  };

  if (error && !scene) return <div className="panel error small">{error}</div>;
  if (!scene) return <div className="preview muted">Reading the scene…</div>;

  const pixel = view && svgRef.current ? view.w / Math.max(1, svgRef.current.clientWidth) : 1;
  const selectedPlaced = placed.find((p) => p.node.path === selected) ?? placed[0];
  const depth = (n: SceneNode) => (n.path === "." ? 0 : n.path.split("/").length);

  return (
    <div className="scene-editor">
      {!editable && (
        <div className="notice small">This copy is read-only. Open the editable copy of this scene to make changes.</div>
      )}
      {error && <div className="panel error small">{error}</div>}
      <div className="scene-main">
        <ul className="scene-tree">
          {scene.nodes.map((n) => (
            <li key={n.path}>
              <button
                className={n.path === selectedPlaced?.node.path ? "scene-node selected" : "scene-node"}
                style={{ paddingLeft: 8 + depth(n) * 14 }}
                onClick={() => setSelected(n.path)}
                title={n.nodeType ?? "instanced scene"}
              >
                <Box size={12} className="muted" />
                <span>{n.name}</span>
                <span className="scene-node-type">{n.nodeType ?? "instance"}</span>
              </button>
            </li>
          ))}
        </ul>
        <div className="scene-canvas-wrap">
          <svg
            ref={svgRef}
            className="scene-canvas"
            viewBox={view ? `${view.x} ${view.y} ${view.w} ${view.h}` : "0 0 400 300"}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onWheel={onWheel}
          >
            {placed.map((p) => {
              const isSelected = p.node.path === selectedPlaced?.node.path;
              const origin = apply(p.global, { x: 0, y: 0 });
              return (
                <g key={p.node.path} onPointerDown={(e) => startNodeDrag(e, p)} className={editable ? "scene-item draggable" : "scene-item"}>
                  {p.rect && (
                    <g transform={`matrix(${p.global.join(" ")})`}>
                      {p.texture ? (
                        <image
                          href={p.texture.src}
                          x={p.rect.x}
                          y={p.rect.y}
                          width={p.rect.w}
                          height={p.rect.h}
                          preserveAspectRatio="none"
                          style={{ imageRendering: "pixelated" }}
                        />
                      ) : (
                        <rect x={p.rect.x} y={p.rect.y} width={p.rect.w} height={p.rect.h} className="scene-box" />
                      )}
                      {isSelected && (
                        <rect
                          x={p.rect.x}
                          y={p.rect.y}
                          width={p.rect.w}
                          height={p.rect.h}
                          className="scene-selection"
                          vectorEffect="non-scaling-stroke"
                        />
                      )}
                    </g>
                  )}
                  <circle cx={origin.x} cy={origin.y} r={(isSelected ? 6 : 4) * pixel} className={isSelected ? "scene-origin selected" : "scene-origin"} />
                  {(isSelected || !p.rect) && (
                    <text x={origin.x + 8 * pixel} y={origin.y - 6 * pixel} fontSize={12 * pixel} className="scene-label">
                      {p.node.name}
                    </text>
                  )}
                </g>
              );
            })}
          </svg>
          <div className="scene-toolbar">
            <button className="icon-button small-icon" onClick={fit} title="Fit the scene in view">
              <Maximize size={15} />
            </button>
            <span className="muted tiny">
              {editable ? "Drag nodes to move them. " : ""}Wheel zooms, drag the background to pan.
            </span>
          </div>
        </div>
      </div>

      {selectedPlaced && (
        <NodeProperties
          key={`${selectedPlaced.node.path}:${revision}`}
          scene={scene}
          node={selectedPlaced.node}
          control={selectedPlaced.control}
          editable={editable}
          onSave={(changes) => save(selectedPlaced.node.path, changes)}
        />
      )}
      <p className="muted tiny">
        Changes are saved to this .tscn right away as edits: Undo reverts them, and the original is kept. 2D transforms
        and sizes are shown; anchors, cameras and scripts' runtime behavior aren't simulated.
      </p>
    </div>
  );
}

function NodeProperties(props: {
  scene: Scene;
  node: SceneNode;
  control: boolean;
  editable: boolean;
  onSave: (changes: [string, string][]) => void;
}) {
  const { scene, node, control, editable, onSave } = props;
  const instance = node.instance ? scene.resources.find((r) => r.id === node.instance) : null;
  const missing = control ? [] : NODE2D_DEFAULTS.filter(([k]) => !node.properties.some((p) => p.key === k));
  return (
    <div className="card scene-props">
      <h4>
        {node.name} <span className="muted small">{node.nodeType ?? "instanced scene"}</span>
      </h4>
      {instance && <p className="muted small">Instance of {instance.path}</p>}
      {node.properties.length === 0 && <p className="muted small">No properties set (everything is at its default).</p>}
      <dl className="prop-list">
        {node.properties.map((p) => (
          <PropertyRow key={p.key} prop={p} editable={editable} onCommit={(v) => onSave([[p.key, v]])} />
        ))}
      </dl>
      {editable && missing.length > 0 && (
        <div className="row add-props">
          <span className="muted small">Add:</span>
          {missing.map(([k, v]) => (
            <button key={k} onClick={() => onSave([[k, v]])}>
              {k}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function PropertyRow({ prop, editable, onCommit }: { prop: SceneProperty; editable: boolean; onCommit: (v: string) => void }) {
  const vector = parseVector2(prop.value);
  const [x, setX] = useState(vector ? formatNumber(vector.x) : "");
  const [y, setY] = useState(vector ? formatNumber(vector.y) : "");
  const [text, setText] = useState(prop.value);
  const commitText = () => text.trim() !== prop.value && text.trim() !== "" && onCommit(text.trim());
  const commitVector = () => {
    const nx = Number(x);
    const ny = Number(y);
    if (!Number.isFinite(nx) || !Number.isFinite(ny) || !vector) return;
    if (nx !== vector.x || ny !== vector.y) onCommit(formatVector2({ x: nx, y: ny }));
  };
  const onEnter = (commit: () => void) => (e: React.KeyboardEvent) => e.key === "Enter" && commit();

  let editor;
  if (prop.value.includes("\n")) {
    editor = <pre className="code prop-multiline">{prop.value}</pre>;
  } else if (vector) {
    editor = (
      <span className="vector-inputs">
        <label>
          x <input type="number" value={x} disabled={!editable} onChange={(e) => setX(e.target.value)} onBlur={commitVector} onKeyDown={onEnter(commitVector)} />
        </label>
        <label>
          y <input type="number" value={y} disabled={!editable} onChange={(e) => setY(e.target.value)} onBlur={commitVector} onKeyDown={onEnter(commitVector)} />
        </label>
      </span>
    );
  } else if (prop.value === "true" || prop.value === "false") {
    editor = (
      <input
        type="checkbox"
        checked={prop.value === "true"}
        disabled={!editable}
        onChange={(e) => onCommit(e.target.checked ? "true" : "false")}
      />
    );
  } else {
    editor = (
      <input
        type="text"
        className="mono"
        value={text}
        disabled={!editable}
        onChange={(e) => setText(e.target.value)}
        onBlur={commitText}
        onKeyDown={onEnter(commitText)}
      />
    );
  }
  return (
    <>
      <dt title={`line ${prop.line}`}>{prop.key}</dt>
      <dd>{editor}</dd>
    </>
  );
}
