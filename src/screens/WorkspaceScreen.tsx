import { revealItemInDir } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  Box,
  ChevronDown,
  ChevronRight,
  Code2,
  Columns2,
  Eye,
  EyeOff,
  Flag,
  FolderOpen,
  Gamepad2,
  Grid3x3,
  Hammer,
  Magnet,
  Map,
  MousePointer2,
  Move,
  Package,
  Pencil,
  Play,
  Plus,
  Replace,
  RotateCw,
  Scaling,
  Shapes,
  SlidersHorizontal,
  Sparkles,
  Trash2,
  Undo2,
  User,
  X,
} from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import AssetBrowser from "../components/gx/AssetBrowser";
import { BehaviorView, EntityInspector, MultiInspector, ThingInspector } from "../components/gx/GxInspector";
import type { GizmoMode } from "../components/gx/SceneView";
import { pickFile } from "../lib/api";
import { gx, type Behavior, type Entity, type Item, type ProjectIndex, type Scene, type Thing, type TranslationReport } from "../lib/gx";
import { allIds, duplicate, findEntity, group, insertEntity, removeEntity, reparent, transformOf, uniqueId, updateEntity, walk } from "../lib/gxscene";

// three.js loads only when a scene is opened.
const SceneView = lazy(() => import("../components/gx/SceneView"));
const Player = lazy(() => import("../components/gx/Player"));

// ---------------------------------------------------------------------------
// The workspace

/** Entities can be selected several at a time (Ctrl/Shift); the last is the main one. */
type Selection = { kind: "entity"; ids: string[] } | { kind: "thing"; id: string } | { kind: "behavior"; id: string } | null;

const one = (id: string): Selection => ({ kind: "entity", ids: [id] });

/** The visual editor for an imported game's layer: opened from that game, never on its own. */
interface WorkspaceProps {
  path: string;
  report?: TranslationReport;
  visible: boolean;
  /** The imported game this layer belongs to. */
  game: string;
  /** Every imported game, for adding one to this layer (Combined Workspace). */
  games: { id: string; name: string }[];
  onClose: () => void;
  /** Opens the game's Build dialog (after the layer's changes were sent into it). */
  onBuild: () => void;
  /** The games panel asked to add a game (Combined Workspace); bumped `n`. */
  addRequest?: { id: string; n: number };
  /** The top bar's Play / Game Projection; bumped. */
  playRequest?: number;
  /** Play as soon as a scene is open (Game Projection chosen before the editor opened). */
  autoplay?: boolean;
  onAutoplayed?: () => void;
  onPlaying?: (playing: boolean) => void;
  /** Play asked for on a scene GameX didn't read from the game (a catalogue, a capture): the real game plays it. */
  onPlayReal?: () => void;
  /** The games in this layer, by their ID in it. */
  onSources?: (ids: string[]) => void;
  /** A line for the status bar. */
  onStatus?: (message: string) => void;
}

export default function Workspace({ path, report, visible, game, games, onClose, onBuild, addRequest, playRequest = 0, autoplay, onAutoplayed, onPlaying, onPlayReal, onSources, onStatus }: WorkspaceProps) {
  const [showReport, setShowReport] = useState(report !== undefined);
  const [index, setIndex] = useState<ProjectIndex | null>(null);
  const [things, setThings] = useState<Record<string, Thing>>({});
  const [behaviors, setBehaviors] = useState<Record<string, Behavior | null>>({});
  const [scene, setScene] = useState<Scene | null>(null);
  const [structure, setStructure] = useState(0);
  const [selection, setSelection] = useState<Selection>(null);
  const [mode, setMode] = useState<GizmoMode>("translate");
  const [snap, setSnap] = useState(true);
  const [showColliders, setShowColliders] = useState(false);
  const [undoLabel, setUndoLabel] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(0);
  const [showProblems, setShowProblems] = useState(false);
  /** The GameX engine is playing the open scene. */
  const [playing, setPlaying] = useState(false);
  /** Bumped when an asset's file changes, so the view reads it again. */
  const [assetsVersion, setAssetsVersion] = useState(0);
  const [exportNote, setExportNote] = useState<string | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  /** Scenes open as tabs, and the second view (another scene, or this 3D one from above). */
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [sideScene, setSideScene] = useState<Scene | null>(null);
  const [sideTab, setSideTab] = useState<"inspector" | "hierarchy">("inspector");
  const mainCursor = useRef<HTMLSpanElement>(null);
  const readAsset = useCallback((asset: string) => gx.readAsset(path, asset), [path]);
  const sideCursor = useRef<HTMLSpanElement>(null);
  // Saves run one after another, in the order they were made.
  const queue = useRef<Promise<unknown>>(Promise.resolve());

  const loadAll = useCallback(
    async (sceneId?: string | null) => {
      const idx = await gx.openProject(path);
      const loaded = await Promise.all(idx.things.map((t) => gx.loadThing(path, t.id).catch(() => null)));
      setIndex(idx);
      setUndoLabel(idx.undo ?? null);
      setThings(Object.fromEntries(loaded.filter((t): t is Thing => t !== null).map((t) => [t.id, t])));
      setBehaviors({});
      const want = sceneId === undefined ? (idx.info.settings.startScene ?? idx.scenes[0]?.id ?? null) : sceneId;
      setScene(want && idx.scenes.some((s) => s.id === want) ? await gx.loadScene(path, want) : null);
      setStructure((n) => n + 1);
    },
    [path],
  );

  useEffect(() => {
    loadAll().catch((e) => setError(String(e)));
  }, [loadAll]);

  // The scene on screen is always one of the tabs.
  useEffect(() => {
    if (scene) setOpenTabs((tabs) => (tabs.includes(scene.id) ? tabs : [...tabs, scene.id]));
  }, [scene?.id]);

  useEffect(() => {
    if (playRequest && scene) play();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playRequest]);
  // The app's callbacks change every render: called through a ref, only when what they report changes.
  const report_ = useRef({ onAutoplayed, onPlaying, onPlayReal, onSources, onStatus });
  report_.current = { onAutoplayed, onPlaying, onPlayReal, onSources, onStatus };
  useEffect(() => {
    if (autoplay && scene) {
      play();
      report_.current.onAutoplayed?.();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoplay, scene?.id]);
  useEffect(() => report_.current.onPlaying?.(playing), [playing]);
  const sourceIds = (index?.info.sources ?? []).map((s) => s.id).join("|");
  useEffect(() => {
    if (index) report_.current.onSources?.(sourceIds ? sourceIds.split("|") : []);
  }, [sourceIds]);
  useEffect(() => {
    if (saving > 0) report_.current.onStatus?.("Saving…");
  }, [saving]);
  useEffect(() => {
    if (exportNote) report_.current.onStatus?.(exportNote);
  }, [exportNote]);
  useEffect(() => {
    if (error) report_.current.onStatus?.(error);
  }, [error]);

  // Behaviors are read when something that uses them is selected.
  const neededBehaviors = useMemo(() => {
    if (!selection) return [];
    if (selection.kind === "behavior") return [selection.id];
    const entity = selection.kind === "entity" && scene && selection.ids.length === 1 ? findEntity(scene.entities, selection.ids[0]) : undefined;
    const thing = selection.kind === "thing" ? things[selection.id] : entity?.thing ? things[entity.thing] : undefined;
    return [...(entity?.behaviors ?? []), ...(thing?.behaviors ?? [])].map((b) => b.behavior);
  }, [selection, scene, things]);
  useEffect(() => {
    for (const id of neededBehaviors) {
      if (id in behaviors) continue;
      setBehaviors((b) => ({ ...b, [id]: undefined as unknown as null }));
      gx.loadBehavior(path, id)
        .then((b) => setBehaviors((all) => ({ ...all, [id]: b })))
        .catch(() => setBehaviors((all) => ({ ...all, [id]: null })));
    }
  }, [neededBehaviors, behaviors, path]);

  /** Scenes GameX didn't read from the game's own levels: a sprite catalogue, a captured level. */
  const notALevel = (s: Scene | null) => !!s && (s.id === "catalogue" || s.id === "things" || s.id.startsWith("captured"));

  /** The GameX engine plays levels it read; a game whose levels are in its code plays for real. */
  const play = () => {
    if (notALevel(scene)) return report_.current.onPlayReal?.();
    setPlaying(true);
  };

  const save = (task: () => Promise<string | null>) => {
    setSaving((n) => n + 1);
    queue.current = queue.current
      .then(task)
      .then((label) => {
        setUndoLabel(label);
        setError(null);
      })
      .catch(async (e) => {
        setError(`Not saved: ${e}`);
        // Show what's really on disk.
        await loadAll(scene?.id ?? null).catch(() => {});
      })
      .finally(() => setSaving((n) => n - 1));
  };

  const commitScene = (next: Scene, summary: string, structural: boolean) => {
    setScene(next);
    if (structural) setStructure((n) => n + 1);
    save(() => gx.saveScene(path, next, summary));
  };

  const changeEntity = (id: string, next: Entity, summary: string) => {
    if (!scene) return;
    const old = findEntity(scene.entities, id);
    const structural = !old || JSON.stringify(old.parts) !== JSON.stringify(next.parts) || old.thing !== next.thing;
    commitScene({ ...scene, entities: updateEntity(scene.entities, id, () => next) }, summary, structural);
  };

  const changeThing = (next: Thing, summary: string) => {
    const kindChanged = things[next.id]?.kind !== next.kind;
    setThings((all) => ({ ...all, [next.id]: next }));
    setStructure((n) => n + 1);
    save(async () => {
      const label = await gx.saveThing(path, next, summary);
      if (kindChanged) setIndex(await gx.openProject(path));
      return label;
    });
  };

  const selectedIds = selection?.kind === "entity" ? selection.ids : [];
  const selectedEntities = scene ? selectedIds.map((id) => findEntity(scene.entities, id)).filter((e): e is Entity => !!e) : [];
  const selectedEntity = selectedEntities.length === 1 ? selectedEntities[0] : undefined;
  const label = (list: Entity[]) => (list.length === 1 ? list[0].name : `${list.length} things`);

  /** Ctrl/Shift adds to or takes from the selection; a plain click replaces it. */
  const select = (id: string | null, add = false) => {
    if (!id) return add ? undefined : setSelection(null);
    if (!add || selection?.kind !== "entity") return setSelection(one(id));
    const ids = selection.ids.includes(id) ? selection.ids.filter((x) => x !== id) : [...selection.ids, id];
    setSelection(ids.length ? { kind: "entity", ids } : null);
  };

  const deleteSelected = () => {
    if (!scene || selectedEntities.length === 0) return;
    setSelection(null);
    const entities = selectedEntities.reduce((list, e) => removeEntity(list, e.id), scene.entities);
    commitScene({ ...scene, entities }, `Delete ${label(selectedEntities)}`, true);
  };

  const duplicateSelected = () => {
    if (!scene || selectedEntities.length === 0) return;
    const taken = allIds(scene.entities);
    let entities = scene.entities;
    const copies: string[] = [];
    for (const e of selectedEntities) {
      const copy = duplicate(e, taken);
      const t = transformOf(copy);
      copy.transform = { ...t, position: [t.position[0] + 1, t.position[1], t.position[2]] };
      entities = insertEntity(entities, copy, e.id);
      copies.push(copy.id);
    }
    commitScene({ ...scene, entities }, `Duplicate ${label(selectedEntities)}`, true);
    setSelection({ kind: "entity", ids: copies });
  };

  const groupSelected = () => {
    if (!scene || selectedEntities.length === 0) return;
    const grouped = group(scene.entities, selectedEntities.map((e) => e.id));
    if (!grouped) return;
    commitScene({ ...scene, entities: grouped.entities }, `Group ${label(selectedEntities)}`, true);
    setSelection(one(grouped.id));
  };

  /** Drag in the outline: under `parent`, or to the top level when null. */
  const moveUnder = (ids: string[], parent: string | null) => {
    if (!scene) return;
    const moved = reparent(scene.entities, ids, parent);
    if (!moved) return setError("Something can't be put inside itself or inside one of its own parts.");
    const names = ids.map((id) => findEntity(scene.entities, id)).filter((e): e is Entity => !!e);
    const into = parent ? findEntity(scene.entities, parent)?.name : null;
    commitScene({ ...scene, entities: moved }, into ? `Put ${label(names)} in ${into}` : `Take ${label(names)} out`, true);
  };

  const addEntity = (entity: Entity, summary: string) => {
    if (!scene) return;
    commitScene({ ...scene, entities: [...scene.entities, entity] }, summary, true);
    setSelection(one(entity.id));
  };

  /** An image (as a sprite) or a model from the asset browser, at the middle of the scene. */
  const addAsset = (asset: string, size?: [number, number]) => {
    if (!scene || !index) return;
    const file = asset.split("/").pop() ?? asset;
    const name = file.replace(/\.[^.]+$/, "");
    const id = uniqueId(name, allIds(scene.entities));
    const ppu = index.info.settings.pixelsPerUnit || 100;
    const isModel = /\.(glb|gltf|obj)$/i.test(asset);
    const part: Entity["parts"] = isModel
      ? [{ type: "model", asset }]
      : [{ type: "sprite", asset, size: size ? [size[0] / ppu, size[1] / ppu] : [1, 1] }];
    addEntity({ id, name, parts: part }, `Add ${file}`);
  };

  const place = (thing: Thing) => {
    if (!scene) return;
    const id = uniqueId(thing.id, allIds(scene.entities));
    addEntity({ id, name: thing.name, thing: thing.id }, `Place ${thing.name}`);
  };

  const replaceAsset = async (asset: string) => {
    const ext = asset.split(".").pop()?.toLowerCase() ?? "";
    const images = ["png", "jpg", "jpeg", "webp"];
    const file = await pickFile("Replacement", images.includes(ext) ? images : [ext]);
    if (!file) return;
    save(async () => {
      const label = await gx.replaceAsset(path, asset, file);
      setAssetsVersion((n) => n + 1);
      return label;
    });
  };

  /** Combined Workspace: another imported game's scenes and things join this layer. */
  const addGame = async (id: string) => {
    const name = games.find((g) => g.id === id)?.name ?? id;
    setAdding(name);
    try {
      await queue.current;
      const { merged } = await gx.addGame(path, id);
      await loadAll(scene?.id ?? null);
      const renamed = merged.renamed.length ? ` ${merged.renamed.length} name${merged.renamed.length === 1 ? " was" : "s were"} taken, so ${name}'s got "-${merged.source}" added.` : "";
      setExportNote(`Added ${name}: ${merged.scenes} scene${merged.scenes === 1 ? "" : "s"}, ${merged.things} things, ${merged.assets} assets.${renamed}`);
    } catch (e) {
      const message = String(e);
      setError(
        message.includes("needs-tool:")
          ? `${name} needs a tool first: open it and choose "Edit visually" once, then add it here.`
          : `Couldn't add ${name}: ${message}`,
      );
    } finally {
      setAdding(null);
    }
  };

  /** Output A: the layer's changes into the game, then the game's Build dialog. */
  const exportApk = async () => {
    try {
      await queue.current;
      const sent = await gx.sendToGame(game);
      const parts = [`${sent.written.length} file${sent.written.length === 1 ? "" : "s"} written into the game`];
      if (sent.objects) parts.push(`${sent.objects} moved or changed object${sent.objects === 1 ? "" : "s"} among them`);
      if (sent.notWritable.length) {
        parts.push(`${sent.notWritable.length} not yet: ${sent.notWritable.map(([a, why]) => `${a.split("/").pop()} (${why})`).join("; ")}`);
      }
      setExportNote(parts.join(". ") + ".");
      onBuild();
    } catch (e) {
      setError(`Couldn't send the changes to the game: ${e}`);
    }
  };

  const undo = async () => {
    try {
      await queue.current;
      const [undone] = await gx.undo(path);
      if (!undone) return;
      await loadAll(scene?.id ?? null);
      setAssetsVersion((n) => n + 1);
      setError(null);
    } catch (e) {
      setError(`Couldn't undo: ${e}`);
    }
  };

  // Scenes: rename, delete, start with.
  const renameScene = (item: Item) => {
    const name = window.prompt("Rename the scene", item.name);
    if (!name?.trim() || name.trim() === item.name) return;
    save(async () => {
      const s = scene?.id === item.id ? scene : await gx.loadScene(path, item.id);
      const renamed = { ...s, name: name.trim() };
      if (scene?.id === item.id) setScene(renamed);
      const label = await gx.saveScene(path, renamed, `Rename scene ${item.name}`);
      setIndex(await gx.openProject(path));
      return label;
    });
  };

  const deleteScene = (item: Item) => {
    if (!window.confirm(`Delete the scene "${item.name}"? Undo brings it back.`)) return;
    if (scene?.id === item.id) {
      setScene(null);
      setSelection(null);
    }
    save(async () => {
      const label = await gx.deleteScene(path, item.id, `Delete scene ${item.name}`);
      setIndex(await gx.openProject(path));
      return label;
    });
  };

  const startWith = (item: Item) => {
    if (!index) return;
    const info = { ...index.info, settings: { ...index.info.settings, startScene: item.id } };
    setIndex({ ...index, info });
    save(() => gx.saveInfo(path, info, `Start with ${item.name}`));
  };

  const openScene = async (item: Item) => {
    try {
      setPlaying(false);
      setSelection(null);
      setScene(await gx.loadScene(path, item.id));
      setStructure((n) => n + 1);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    if (addRequest?.n) addGame(addRequest.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [addRequest?.n]);

  const closeTab = (id: string) => {
    const rest = openTabs.filter((t) => t !== id);
    setOpenTabs(rest);
    if (sideScene?.id === id && scene?.id !== id) setSideScene(null);
    if (scene?.id !== id) return;
    const next = index?.scenes.find((s) => s.id === rest[rest.length - 1]);
    if (next) openScene(next);
    else {
      setScene(null);
      setSelection(null);
    }
  };

  /** A second view: a 3D level from above, or a 2D scene beside another (another game's first). */
  const openSplit = async () => {
    if (!scene || !index) return;
    if (scene.dimensions === "3d") return setSideScene(scene);
    const others = index.scenes.filter((s) => s.id !== scene.id && s.kind === "2d");
    const pick = others.find((s) => !openTabs.includes(s.id)) ?? others[0];
    if (!pick) return setError("There's no other 2D scene to show beside this one. Add a game to the workspace, or make a scene.");
    try {
      setSideScene(await gx.loadScene(path, pick.id));
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleVisible = (id: string) => {
    const e = scene && findEntity(scene.entities, id);
    if (!e) return;
    const shown = e.active !== false;
    changeEntity(id, { ...e, active: !shown }, `${shown ? "Hide" : "Show"} ${e.name}`);
  };

  // Keys, while the workspace is on screen and not typing in a field (the player has its own).
  useEffect(() => {
    if (!visible || playing) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.closest("input, textarea, select")) return;
      const k = e.key.toLowerCase();
      if ((e.ctrlKey || e.metaKey) && k === "z") {
        e.preventDefault();
        undo();
      } else if ((e.ctrlKey || e.metaKey) && k === "d") {
        e.preventDefault();
        duplicateSelected();
      } else if ((e.ctrlKey || e.metaKey) && k === "g") {
        e.preventDefault();
        groupSelected();
      } else if (e.key === "Delete") {
        deleteSelected();
      } else if (!e.ctrlKey && !e.metaKey && !e.altKey) {
        if (k === "q") setMode("select");
        if (k === "w") setMode("translate");
        if (k === "e") setMode("rotate");
        if (k === "r") setMode("scale");
        if (k === "escape") setSelection(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!index) {
    return (
      <div className="screen">
        {error ? (
          <>
            <div className="panel error">{error}</div>
            <button onClick={onClose}>Back to projects</button>
          </>
        ) : (
          <p className="muted">Opening the project…</p>
        )}
      </div>
    );
  }

  const instanceCount = (thingId: string) => (scene ? walk(scene.entities).filter((w) => w.entity.thing === thingId).length : 0);
  const thingsByKind = groupBy(index.things, (t) => t.kind ?? "other");
  // The second pane: the same 3D level seen from above, or another scene beside this one.
  const sideSame = !!scene && !!sideScene && sideScene.id === scene.id;
  const shownSide = sideSame ? scene : sideScene;

  /** The asset the selection is drawn with (its sprite or model), if any. */
  const selectedLook = (() => {
    const parts = [...(selectedEntity?.parts ?? []), ...(selectedEntity?.thing ? (things[selectedEntity.thing]?.parts ?? []) : []), ...(selection?.kind === "thing" ? (things[selection.id]?.parts ?? []) : [])];
    const p = parts.find((x) => x.type === "sprite" || x.type === "model");
    return p && "asset" in p ? p.asset : undefined;
  })();

  const quickReplace = () => {
    if (selectedLook) return replaceAsset(selectedLook);
    document.getElementById("gx-assets")?.scrollIntoView({ behavior: "smooth" });
    setExportNote("Pick something in the scene first, or use Replace on a file in the assets below.");
  };

  const quickProperties = () => {
    setSideTab("inspector");
    if (selectedEntity?.thing) return setSelection({ kind: "thing", id: selectedEntity.thing });
    const player = Object.values(things).find((t) => t.role === "player") ?? Object.values(things)[0];
    if (player) setSelection({ kind: "thing", id: player.id });
  };

  const pane = (opts: { side: boolean }) => {
    const s = opts.side ? shownSide : scene;
    if (!s) return null;
    const editable = !opts.side || sideSame;
    const top = opts.side && sideSame && s.dimensions === "3d";
    const title = s.dimensions === "2d" ? "2D View" : top ? "Top View (2D)" : "3D View";
    return (
      <section className="gx-pane" key={opts.side ? "side" : "main"}>
        <div className="gx-pane-head">
          <strong className="small">{title}</strong>
          {opts.side ? (
            <select
              className="gx-pane-scene"
              value={sideScene?.id ?? ""}
              onChange={(e) => {
                const item = index.scenes.find((x) => x.id === e.target.value);
                if (item) gx.loadScene(path, item.id).then(setSideScene).catch((err) => setError(String(err)));
              }}
            >
              {index.scenes.map((x) => (
                <option key={x.id} value={x.id}>
                  {x.id === scene?.id ? `${x.name} (seen from above)` : x.name}
                </option>
              ))}
            </select>
          ) : (
            <span className="muted small">{s.name}</span>
          )}
          <span className="status-spacer" />
          {opts.side ? (
            <button className="icon-button gx-mini" title="Close this view" onClick={() => setSideScene(null)}>
              <X size={13} />
            </button>
          ) : (
            <button className="icon-button gx-mini" title="Split: a second view beside this one" onClick={openSplit} disabled={!!sideScene}>
              <Columns2 size={13} />
            </button>
          )}
        </div>
        <div className="gx-pane-body">
          {editable && !playing && (
            <div className="gx-tools">
              <button className={mode === "select" ? "active" : ""} onClick={() => setMode("select")} title="Select (Q)">
                <MousePointer2 size={16} />
              </button>
              <button className={mode === "translate" ? "active" : ""} onClick={() => setMode("translate")} title="Move (W)">
                <Move size={16} />
              </button>
              <button className={mode === "rotate" ? "active" : ""} onClick={() => setMode("rotate")} title="Turn (E)">
                <RotateCw size={16} />
              </button>
              <button className={mode === "scale" ? "active" : ""} onClick={() => setMode("scale")} title="Resize (R)">
                <Scaling size={16} />
              </button>
              <button onClick={deleteSelected} disabled={selectedEntities.length === 0} title="Delete (Del)">
                <Trash2 size={16} />
              </button>
              <hr />
              <button className={snap ? "active" : ""} onClick={() => setSnap((v) => !v)} title="Snap: 0.25 m, 15°">
                <Magnet size={16} />
              </button>
              <button className={showColliders ? "active" : ""} onClick={() => setShowColliders((v) => !v)} title="Show colliders (green; triggers yellow)">
                <Grid3x3 size={16} />
              </button>
            </div>
          )}
          <main className="gx-view">
            <SceneView
              key={`${opts.side ? "side" : "main"}-${assetsVersion}-${top ? "top" : "persp"}`}
              scene={s}
              things={things}
              structure={structure}
              selected={editable ? selectedIds : []}
              mode={editable ? mode : "select"}
              snap={snap}
              showColliders={showColliders}
              top={top}
              active={visible && !playing}
              cursor={opts.side ? sideCursor : mainCursor}
              toolbar={
                editable ? (
                  <>
                    <button title="Move (W)" onClick={() => setMode("translate")}>
                      <Move size={14} />
                    </button>
                    <button title="Turn (E)" onClick={() => setMode("rotate")}>
                      <RotateCw size={14} />
                    </button>
                    <button title="Delete (Del)" onClick={deleteSelected}>
                      <Trash2 size={14} />
                    </button>
                  </>
                ) : null
              }
              readAsset={readAsset}
              onSelect={(id, add) => {
                if (editable) return select(id, add);
                // Another scene: clicking it makes it the one being edited (and this one moves aside).
                const item = index.scenes.find((x) => x.id === s.id);
                if (item && scene) {
                  setSideScene(scene);
                  openScene(item);
                }
              }}
              onTransforms={(changes) => {
                if (!editable || !scene) return;
                // A drag that ended where it started changes nothing.
                const real = changes.filter((c) => {
                  const e = findEntity(scene.entities, c.id);
                  return e && JSON.stringify(transformOf(e)) !== JSON.stringify(c.transform);
                });
                if (real.length === 0) return;
                const moved = real.map((c) => findEntity(scene.entities, c.id)!);
                const entities = real.reduce((list, c) => updateEntity(list, c.id, (e) => ({ ...e, transform: c.transform })), scene.entities);
                const verb = mode === "translate" ? "Move" : mode === "rotate" ? "Turn" : "Resize";
                commitScene({ ...scene, entities }, `${verb} ${label(moved)}`, false);
              }}
            />
            {!opts.side && playing && (
              <Player
                scene={s}
                things={things}
                pixelsPerUnit={index.info.settings.pixelsPerUnit || 100}
                readAsset={readAsset}
                loadBehavior={(id) => gx.loadBehavior(path, id)}
                findScene={async (name) => {
                  const item = index.scenes.find((x) => x.id === name) ?? index.scenes.find((x) => x.name.toLowerCase() === name.toLowerCase());
                  return item ? gx.loadScene(path, item.id) : null;
                }}
                onStop={() => setPlaying(false)}
              />
            )}
          </main>
        </div>
        <div className="gx-pane-foot muted small">
          <span ref={opts.side ? sideCursor : mainCursor} className="mono">
            X: – Y: –
          </span>
          <span className="status-spacer" />
          <span>{s.dimensions === "2d" || top ? "Drag to pan · scroll to zoom · F frames" : "Drag to orbit · right-drag to pan · F frames"}</span>
        </div>
      </section>
    );
  };

  const kindTitles: Record<string, string> = { character: "Characters", item: "Items", tool: "Tools", prop: "Props", other: "Other" };

  return (
    <div className="gx-workspace">
      {error && (
        <div className="panel error small gx-error">
          {error}{" "}
          <button className="link" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}
      {exportNote && (
        <div className="notice small gx-problems">
          <Package size={14} />
          <span className="grow">{exportNote}</span>
          <button className="link" onClick={() => setExportNote(null)}>
            Dismiss
          </button>
        </div>
      )}
      {report && showReport && (
        <div className="notice small gx-problems">
          <Gamepad2 size={14} />
          <span className="grow">
            Translated from {report.engine || `Godot ${report.godotVersion}`}: {report.scenes} scene{report.scenes === 1 ? "" : "s"}, {report.things}{" "}
            things, {report.entities} objects, {report.assets} assets. {report.scripts} script{report.scripts === 1 ? "" : "s"} kept by name (not
            translated yet).
            {Object.keys(report.untranslated).length > 0 && (
              <>
                {" "}
                Kept but not translated yet:{" "}
                {Object.entries(report.untranslated)
                  .sort((a, b) => b[1] - a[1])
                  .map(([k, n]) => `${k} (${n})`)
                  .join(", ")}
                .
              </>
            )}
            {report.problems.map((p) => (
              <span key={p} className="bad"> {p}</span>
            ))}
          </span>
          <button className="link" onClick={() => setShowReport(false)}>
            Dismiss
          </button>
        </div>
      )}
      {scene && notALevel(scene) && (
        <div className="notice small gx-problems">
          <AlertTriangle size={14} />
          <span className="grow">
            {scene.id.startsWith("captured")
              ? "A level captured from the running game: its look and objects, to edit. Its gameplay is the real game's: Play runs the real game."
              : "Not a level: this game builds its levels in its code, so this is its sprite catalogue. Play the real game in Live Preview, then use Capture level there to bring a level in."}
          </span>
          <button className="link" onClick={() => report_.current.onPlayReal?.()}>
            Open Live Preview
          </button>
        </div>
      )}
      {index.problems.length > 0 && (
        <div className="notice small gx-problems">
          <AlertTriangle size={14} />
          <button className="link" onClick={() => setShowProblems((v) => !v)}>
            {index.problems.length} problem{index.problems.length === 1 ? "" : "s"} in this project
          </button>
          {showProblems && (
            <ul>
              {index.problems.map((p, i) => (
                <li key={i}>
                  <span className="mono">{p.file}</span>: {p.message}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      <div className="gx-body">
        <aside className="gx-browser">
          <div className="gx-ws-head">
            <strong className="grow" title={index.path}>
              {index.info.name}
            </strong>
            <button className="icon-button gx-mini" onClick={() => revealItemInDir(index.path).catch(() => {})} title="Show the workspace's folder">
              <FolderOpen size={13} />
            </button>
            <button className="icon-button gx-mini" onClick={onClose} title="Back to the game's files">
              <X size={13} />
            </button>
          </div>
          {adding && <p className="muted small">Adding {adding}…</p>}
          <Section title="Maps / Levels" count={index.scenes.length} icon={<Map size={14} />}>
            {index.scenes.length === 0 && <p className="muted small">No scenes yet.</p>}
            {index.scenes.map((s) => {
              const start = index.info.settings.startScene === s.id;
              return (
                <div key={s.id} className={scene?.id === s.id ? "gx-item gx-scene active" : "gx-item gx-scene"}>
                  <button className="gx-outline-name" onClick={() => openScene(s)} onDoubleClick={() => renameScene(s)} title="Double-click to rename">
                    {s.name}
                  </button>
                  <span className="tag">{s.kind}</span>
                  <button className={start ? "icon-button gx-mini on" : "icon-button gx-mini"} title={start ? "Play starts here" : "Start the game here"} onClick={() => startWith(s)}>
                    <Flag size={12} />
                  </button>
                  <button className="icon-button gx-mini" title="Rename" onClick={() => renameScene(s)}>
                    <Pencil size={12} />
                  </button>
                  <button className="icon-button gx-mini" title="Delete this scene" onClick={() => deleteScene(s)}>
                    <Trash2 size={12} />
                  </button>
                </div>
              );
            })}
          </Section>
          {["character", "item", "tool", "prop", "other"]
            .filter((k) => thingsByKind[k]?.length)
            .map((kind) => (
              <Section key={kind} title={kindTitles[kind]} count={thingsByKind[kind].length} icon={kindIcon(kind)} closed={kind === "other"}>
                {thingsByKind[kind].map((t) => (
                  <button
                    key={t.id}
                    className={selection?.kind === "thing" && selection.id === t.id ? "gx-item active" : "gx-item"}
                    onClick={() => {
                      setSideTab("inspector");
                      setSelection({ kind: "thing", id: t.id });
                    }}
                  >
                    <span className="grow">{t.name}</span>
                    {things[t.id]?.role && <span className="tag">{things[t.id].role}</span>}
                    {scene && <span className="count">{instanceCount(t.id) || ""}</span>}
                  </button>
                ))}
              </Section>
            ))}
          <Section title="Behavior" count={index.behaviors.length + index.scripts.length} icon={<Code2 size={14} />} closed>
            {index.behaviors.map((b) => (
              <button key={b.id} className={selection?.kind === "behavior" && selection.id === b.id ? "gx-item active" : "gx-item"} onClick={() => setSelection({ kind: "behavior", id: b.id })}>
                {b.name}
              </button>
            ))}
            {index.scripts.map((s) => (
              <div key={s} className="gx-item muted">
                {s.split("/").pop()}
              </div>
            ))}
          </Section>
          <div className="gx-quick">
            <div className="gx-quick-head">Quick Actions</div>
            <button onClick={quickReplace} title={selectedLook ? `Replace ${selectedLook.split("/").pop()}` : "Replace an image or sound"}>
              <Replace size={15} /> Replace Asset
            </button>
            <button onClick={quickProperties}>
              <SlidersHorizontal size={15} /> Edit Properties
            </button>
            <label className="gx-quick-add" title="Put one of the game's things in the scene">
              <Plus size={15} />
              <select value="" disabled={!scene || index.things.length === 0} onChange={(e) => things[e.target.value] && place(things[e.target.value])}>
                <option value="">Add New…</option>
                {index.things.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            </label>
            <button onClick={exportApk}>
              <Package size={15} /> Export Modified APK
            </button>
          </div>
        </aside>

        <div className="gx-center">
          <div className="gx-tabs">
            {openTabs.map((id) => {
              const item = index.scenes.find((s) => s.id === id);
              if (!item) return null;
              return (
                <div key={id} className={scene?.id === id ? "gx-tab active" : "gx-tab"}>
                  <button className="gx-tab-name" onClick={() => openScene(item)}>
                    {item.kind === "3d" ? <Box size={13} /> : <Map size={13} />} {item.name}
                  </button>
                  <button className="gx-tab-close" title="Close" onClick={() => closeTab(id)}>
                    <X size={12} />
                  </button>
                </div>
              );
            })}
            <select className="gx-tab-add" value="" onChange={(e) => { const item = index.scenes.find((s) => s.id === e.target.value); if (item) openScene(item); }} title="Open another scene">
              <option value="">+</option>
              {index.scenes.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.name}
                </option>
              ))}
            </select>
            <span className="status-spacer" />
            <span className="muted small gx-save-state">{saving > 0 ? "Saving…" : "Saved"}</span>
            <button className="icon-button" onClick={undo} disabled={!undoLabel} title={undoLabel ? `Undo: ${undoLabel} (Ctrl+Z)` : "Nothing to undo"}>
              <Undo2 size={16} />
            </button>
            {scene && (
              <button
                className={playing ? "primary gx-play active" : "primary gx-play"}
                onClick={() => (playing ? setPlaying(false) : play())}
                title={playing ? "Stop (Esc)" : notALevel(scene) ? "Play the real game (this game's levels are built in its code)" : "Test play with the GameX engine (generic rules, not the original game's)"}
              >
                {playing ? <X size={15} /> : <Play size={15} />} {playing ? "Stop" : "Play"}
              </button>
            )}
          </div>
          <div className={shownSide && !playing ? "gx-panes split" : "gx-panes"}>
            {scene ? (
              <Suspense fallback={<div className="gx-canvas muted center">Loading the view…</div>}>
                {pane({ side: false })}
                {shownSide && !playing && pane({ side: true })}
              </Suspense>
            ) : (
              <div className="gx-pane gx-canvas muted center">{index.scenes.length ? "Pick a map or level on the left." : "This project has no scenes yet."}</div>
            )}
          </div>
          {!playing && (
            <AssetBrowser
              assets={index.assets}
              things={Object.values(things)}
              readAsset={readAsset}
              version={assetsVersion}
              canAdd={scene !== null}
              onReplace={replaceAsset}
              onAdd={addAsset}
              onPlace={place}
              onOpenThing={(id) => {
                setSideTab("inspector");
                setSelection({ kind: "thing", id });
              }}
            />
          )}
        </div>

        <aside className="gx-side">
          <div className="gx-side-tabs">
            <button className={sideTab === "inspector" ? "active" : ""} onClick={() => setSideTab("inspector")}>
              Inspector
            </button>
            <button className={sideTab === "hierarchy" ? "active" : ""} onClick={() => setSideTab("hierarchy")}>
              Scene Hierarchy
            </button>
          </div>
          {sideTab === "hierarchy" ? (
            scene ? (
              <div className="gx-hierarchy">
                <div className="gx-hierarchy-head small">
                  <strong className="grow">{scene.name}</strong>
                  <span className="count">{walk(scene.entities).length}</span>
                </div>
                <Outline entities={scene.entities} things={things} selected={selectedIds} onSelect={select} onMove={moveUnder} onToggle={toggleVisible} />
                <div
                  className="gx-drop-top muted tiny"
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={(e) => {
                    e.preventDefault();
                    const ids = dragged(e);
                    if (ids.length) moveUnder(ids, null);
                  }}
                >
                  Drag here to take something out of its parent. Ctrl+click picks several.
                </div>
              </div>
            ) : (
              <p className="muted small gx-inspector">Open a map or level to see what's in it.</p>
            )
          ) : selectedEntity ? (
            <EntityInspector
              key={selectedEntity.id}
              entity={selectedEntity}
              thing={selectedEntity.thing ? things[selectedEntity.thing] : undefined}
              behaviors={behaviors}
              onChange={(next, summary) => changeEntity(selectedEntity.id, next, summary)}
              onOpenThing={(id) => setSelection({ kind: "thing", id })}
              onDuplicate={duplicateSelected}
              onDelete={deleteSelected}
              readAsset={readAsset}
              onReplace={replaceAsset}
            />
          ) : selectedEntities.length > 1 ? (
            <MultiInspector entities={selectedEntities} onGroup={groupSelected} onDuplicate={duplicateSelected} onDelete={deleteSelected} />
          ) : selection?.kind === "thing" && things[selection.id] ? (
            <ThingInspector
              key={selection.id}
              thing={things[selection.id]}
              instances={instanceCount(selection.id)}
              behaviors={behaviors}
              canPlace={scene !== null}
              onChange={changeThing}
              onPlace={() => place(things[selection.id])}
              readAsset={readAsset}
              onReplace={replaceAsset}
            />
          ) : selection?.kind === "behavior" ? (
            <div className="gx-inspector">
              <BehaviorView id={selection.id} behavior={behaviors[selection.id]} />
              <p className="muted tiny">Editing blocks comes with the behavior editor (milestone 4). The rules are shown as they're stored.</p>
            </div>
          ) : (
            <div className="gx-inspector muted small">
              <p>Click something in the scene or the list to see and change it.</p>
              <p>
                <strong>W</strong> move · <strong>E</strong> turn · <strong>R</strong> resize · <strong>F</strong> frame (in the view) ·{" "}
                <strong>Del</strong> delete · <strong>Ctrl+D</strong> duplicate · <strong>Ctrl+G</strong> group ·{" "}
                <strong>Ctrl+click</strong> pick several · <strong>Ctrl+Z</strong> undo
              </p>
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}

function groupBy<T>(items: T[], key: (t: T) => string): Record<string, T[]> {
  const out: Record<string, T[]> = {};
  for (const item of items) (out[key(item)] ??= []).push(item);
  return out;
}

function kindIcon(kind: string) {
  switch (kind) {
    case "character":
      return <User size={14} />;
    case "item":
      return <Sparkles size={14} />;
    case "tool":
      return <Hammer size={14} />;
    default:
      return <Shapes size={14} />;
  }
}

function Section({ title, count, closed, icon, children }: { title: string; count: number; closed?: boolean; icon?: React.ReactNode; children: React.ReactNode }) {
  const [open, setOpen] = useState(!closed);
  return (
    <section className="gx-browser-section">
      <button className="gx-section-head" onClick={() => setOpen((o) => !o)}>
        {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        {icon}
        <span className="grow">{title}</span>
        <span className="count">{count}</span>
      </button>
      {open && <div className="gx-section-body">{children}</div>}
    </section>
  );
}

/** IDs carried by a drag from the outline. */
function dragged(e: React.DragEvent): string[] {
  const raw = e.dataTransfer.getData("application/x-gamex-entities");
  try {
    const ids = JSON.parse(raw);
    return Array.isArray(ids) ? ids.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

interface OutlineProps {
  entities: Entity[];
  things: Record<string, Thing>;
  selected: string[];
  onSelect: (id: string, add: boolean) => void;
  /** Dropped `ids` onto `parent`. */
  onMove: (ids: string[], parent: string | null) => void;
  /** Shows or hides one (the eye). */
  onToggle?: (id: string) => void;
  depth?: number;
}

/** The scene's entities as a tree: click to select, drag onto another to put it inside. */
function Outline({ entities, depth = 0, ...rest }: OutlineProps) {
  return (
    <>
      {entities.map((e) => (
        <OutlineRow key={e.id} entity={e} depth={depth} {...rest} />
      ))}
    </>
  );
}

function OutlineRow({ entity, things, selected, onSelect, onMove, onToggle, depth }: Omit<OutlineProps, "entities"> & { entity: Entity; depth: number }) {
  const hasChildren = (entity.children?.length ?? 0) > 0;
  const containsSelected = hasChildren && selected.some((id) => findEntity(entity.children!, id) !== undefined);
  const [open, setOpen] = useState(false);
  const [over, setOver] = useState(false);
  const shown = open || containsSelected;
  const thing = entity.thing ? things[entity.thing] : undefined;
  const isSelected = selected.includes(entity.id);
  return (
    <>
      <div
        className={`gx-item gx-outline${isSelected ? " active" : ""}${over ? " drop" : ""}`}
        style={{ paddingLeft: 6 + depth * 14 }}
        draggable
        onDragStart={(e) => {
          // Dragging a selected row takes the whole selection along.
          const ids = isSelected ? selected : [entity.id];
          e.dataTransfer.setData("application/x-gamex-entities", JSON.stringify(ids));
          e.dataTransfer.effectAllowed = "move";
        }}
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setOver(false);
          const ids = dragged(e).filter((id) => id !== entity.id);
          if (ids.length) {
            onMove(ids, entity.id);
            setOpen(true);
          }
        }}
        title="Drag onto another to put it inside"
      >
        {hasChildren ? (
          <button className="gx-twisty" onClick={() => setOpen((o) => !o)} aria-label={shown ? "Collapse" : "Expand"}>
            {shown ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
          </button>
        ) : (
          <span className="gx-twisty" />
        )}
        <button className="gx-outline-name" onClick={(e) => onSelect(entity.id, e.ctrlKey || e.shiftKey || e.metaKey)}>
          <span className={entity.active === false ? "muted" : undefined}>{entity.name}</span>
          {thing && <span className="muted tiny"> {thing.kind}</span>}
        </button>
        {onToggle && (
          <button className="icon-button gx-mini gx-eye" title={entity.active === false ? "Show" : "Hide"} onClick={() => onToggle(entity.id)}>
            {entity.active === false ? <EyeOff size={12} /> : <Eye size={12} />}
          </button>
        )}
      </div>
      {hasChildren && shown && (
        <Outline entities={entity.children!} things={things} selected={selected} onSelect={onSelect} onMove={onMove} onToggle={onToggle} depth={depth + 1} />
      )}
    </>
  );
}