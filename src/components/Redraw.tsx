import { Circle, Clapperboard, Eye, EyeOff, Save, Square } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { gx } from "../lib/gx";
import type { GameObject, RecSaved, RecView } from "../lib/types";
import type { Arrow, BoxesMode, SceneBox } from "./GameScreen";
import { errorText } from "../lib/errors";

/** The kinds of boxes, in the order the filter lists them. */
const KINDS: { id: string; label: string }[] = [
  { id: "player", label: "Player" },
  { id: "character", label: "Characters" },
  { id: "item", label: "Items" },
  { id: "object", label: "Objects" },
  { id: "tile", label: "Tiles" },
  { id: "sprite", label: "Sprites" },
  { id: "ui", label: "HUD" },
  { id: "background", label: "Backdrop" },
];

const hiddenKey = (projectId: string) => `gamex.redraw.hidden.${projectId}`;
const arrowKey = (projectId: string) => `gamex.redraw.arrows.${projectId}`;

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : (JSON.parse(v) as T);
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Not remembered across restarts; still works now.
  }
}

const DPAD: Record<Arrow, number> = { up: 19, down: 20, left: 21, right: 22 };
const STEP: Record<Arrow, [number, number]> = { up: [0, -1], down: [0, 1], left: [-1, 0], right: [1, 0] };

interface Options {
  projectId: string;
  visible: boolean;
  connected: boolean;
  /** Connects to the running game (after the recording copy starts). */
  connect: () => Promise<void>;
  /** The game's classes whose objects can be on screen. */
  liveClasses: string[];
  /** The game's classes, for each one's category. */
  classes: GameObject[];
  selectedObject: string | null;
  /** A box of one of the game's objects was clicked. */
  onSelect: (cls: string, object: string) => void;
  onError: (message: string) => void;
  onOpenEditor?: (scene: string) => void;
}

/**
 * Redraw: the running game seen through GameX's draw recorder. Every sprite
 * gets a box (filtered by kind), the player is found and steered with the
 * arrow keys, time can be stopped to look and move things, and the level is
 * recorded while exploring and saved in the Editor.
 */
export function useRedraw({ projectId, visible, connected, connect, liveClasses, classes, selectedObject, onSelect, onError, onOpenEditor }: Options) {
  const [view, setView] = useState<RecView | null>(null);
  const [paused, setPaused] = useState(false);
  const [recording, setRecording] = useState(false);
  const [building, setBuilding] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState<RecSaved | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [showKinds, setShowKinds] = useState(false);
  const [hidden, setHidden] = useState<Set<string>>(() => new Set(load<string[]>(hiddenKey(projectId), ["tile", "background"])));
  const [arrows, setArrows] = useState<"tap" | "dpad">(() => load(arrowKey(projectId), "tap"));
  const busy = useRef(false);
  const restart = useRef(false);

  // Each class's category, for the colour of its boxes.
  const category = useMemo(() => new Map(classes.map((c) => [c.id.slice("class:".length), c.category])), [classes]);

  // About 1.5 looks a second while the Live tab is on screen.
  const classesKey = liveClasses.join("|");
  useEffect(() => {
    if (!connected || !visible) return;
    let stop = false;
    const tick = async () => {
      if (busy.current || stop) return;
      busy.current = true;
      try {
        const v = await api.recTick(projectId, liveClasses, recording, restart.current);
        restart.current = false;
        if (stop) return;
        setView(v);
        setPaused(v.paused);
        if (v.spawned && v.player) {
          const name = v.player[1].split(".").pop();
          setNote(`${name} is the player: GameX found who answers your taps and who the camera follows. Arrow keys or WASD steer them.`);
        }
      } catch (e) {
        onError(errorText(e));
      } finally {
        busy.current = false;
      }
    };
    tick();
    const timer = window.setInterval(tick, 650);
    return () => {
      stop = true;
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connected, visible, projectId, classesKey, recording]);

  useEffect(() => {
    if (!connected) setView(null);
  }, [connected]);

  const kindOf = useCallback(
    (b: RecView["boxes"][number]): string => {
      if (b.player) return "player";
      if (b.kind !== "object") return b.kind;
      const c = b.class ? category.get(b.class) : undefined;
      return c === "characters" ? "character" : c === "items" ? "item" : "object";
    },
    [category],
  );

  const all = view?.recorder ? view.boxes : [];
  const counts = useMemo(() => {
    const n: Record<string, number> = {};
    for (const b of all) n[kindOf(b)] = (n[kindOf(b)] ?? 0) + 1;
    return n;
  }, [all, kindOf]);

  const boxes: BoxesMode | null = view?.recorder
    ? {
        boxes: all
          .map((b, i): SceneBox => {
            const kind = kindOf(b);
            const name = b.class?.split(".").pop() ?? KINDS.find((k) => k.id === kind)?.label ?? kind;
            return {
              key: b.object ?? `draw-${i}`,
              x: b.x,
              y: b.y,
              w: b.w,
              h: b.h,
              kind,
              label: b.player ? `${name} (player)` : name,
              selected: !!b.object && b.object === selectedObject,
              player: b.player,
              movable: paused && !!b.object,
            };
          })
          .filter((b) => !hidden.has(b.kind)),
        onSelect: (key) => {
          const b = all.find((x) => x.object === key);
          if (b?.object && b.class) onSelect(b.class, b.object);
        },
        onMove: (key, dx, dy) => {
          if (!paused) return;
          api.recMove(projectId, key, dx, dy).catch((e) => onError(errorText(e)));
        },
      }
    : null;

  const togglePause = async () => {
    try {
      setPaused(await api.recPause(projectId, !paused));
    } catch (e) {
      onError(errorText(e));
    }
  };

  const onArrow = (dir: Arrow) => {
    if (paused) return;
    if (arrows === "dpad") {
      api.mirrorKey(projectId, DPAD[dir]).catch((e) => onError(errorText(e)));
      return;
    }
    // Tap a little way from the player, the way they should go.
    const p = view?.boxes.find((b) => b.player);
    if (!p) {
      setNote("GameX hasn't found the player yet: tap the game to move them once, then the arrow keys steer them.");
      return;
    }
    const [sx, sy] = STEP[dir];
    const step = Math.max(p.w, p.h) * 1.5;
    const x = Math.max(0, Math.round(p.x + p.w / 2 + sx * step));
    const y = Math.max(0, Math.round(p.y + p.h / 2 + sy * step));
    api.mirrorTouch(projectId, x, y, x, y, 80).catch((e) => onError(errorText(e)));
  };

  /** Builds the recording copy, runs it in place of the game and connects. */
  const buildAndRun = async () => {
    setBuilding("Building a recording copy of the game…");
    const unlisten = await api.onBuildProgress((p) => setBuilding(p.message));
    try {
      const result = await api.recBuild(projectId);
      unlisten();
      setBuilding(`Recorder added (${result.recorder?.draws ?? 0} draw calls). Installing and starting it…`);
      const targets = await api.runTargets();
      const target = targets.find((t) => t.running) ?? targets[0];
      if (!target) throw new Error("No emulator or phone found. Start one (or open Run), then try again.");
      await api.runGame(projectId, target.id, "build", true);
      setBuilding("Connecting…");
      await new Promise((r) => window.setTimeout(r, 2500));
      await connect();
      setNote("The recording copy is running. Play into a level: every sprite gets a box, and Record level redraws the level in the Editor.");
    } catch (e) {
      unlisten();
      onError(errorText(e));
    } finally {
      setBuilding(null);
    }
  };

  const toggleRecording = () => {
    if (!recording) {
      restart.current = true;
      setSaved(null);
    }
    setRecording((r) => !r);
  };

  const save = async () => {
    setSaving(true);
    try {
      const layer = (await gx.layerForImport(projectId))?.path ?? (await gx.makeLayer(projectId)).path;
      const kinds = Object.fromEntries(classes.map((c) => [c.id.slice("class:".length), c.category]));
      setSaved(await api.recSave(projectId, layer, kinds));
      setRecording(false);
    } catch (e) {
      onError(errorText(e));
    } finally {
      setSaving(false);
    }
  };

  const saveHidden = (next: Set<string>) => {
    setHidden(next);
    store(hiddenKey(projectId), [...next]);
  };

  const level = view?.level;
  const panel = (
    <div className="redraw-bar">
      <Clapperboard size={15} />
      {building ? (
        <span className="grow small">{building}</span>
      ) : !connected ? (
        <span className="grow small muted">Connect to the running game to box every sprite on screen and redraw its levels.</span>
      ) : view && !view.recorder ? (
        <>
          <span className="grow small">
            To box every sprite and redraw levels, GameX runs a recording copy of this game: your edits plus GameX's draw recorder. It replaces the
            installed game on the emulator (its saved progress there is lost).
          </span>
          <button className="small primary" onClick={buildAndRun}>
            Build and run recording copy
          </button>
        </>
      ) : view ? (
        <>
          <span className="grow small">
            {note ??
              `${all.length} sprites on screen${view.linked ? ", matched to the game's objects" : view.boxes.length ? ", matching them to the game's objects…" : ""}.` +
                (view.player ? ` Player: ${view.player[1].split(".").pop()}.` : "") +
                (level ? ` Level: ${level.pieces} pieces, ${level.objects} objects over ${level.frames} looks${recording ? " (walk around to add more)" : ""}.` : "") +
                (view.missing ? ` Fetching ${view.missing} picture${view.missing === 1 ? "" : "s"}…` : "")}
            {note && (
              <button className="link" onClick={() => setNote(null)}>
                {" "}OK
              </button>
            )}
          </span>
          <button className={showKinds ? "small active" : "small"} onClick={() => setShowKinds((s) => !s)} title="Choose which kinds of boxes are shown">
            <Eye size={13} /> Boxes
          </button>
          <button
            className={recording ? "small active" : "small"}
            onClick={toggleRecording}
            title={recording ? "Stop adding to the level (it's kept until you record again)" : "Redraw the level as you play: everything you see is added"}
          >
            {recording ? <Square size={12} /> : <Circle size={12} />} {recording ? "Stop" : "Record level"}
          </button>
          <button className="small primary" onClick={save} disabled={saving || !level || level.pieces + level.objects === 0} title="Save the recorded level in the Editor, as a new scene">
            <Save size={13} /> {saving ? "Saving…" : "Save to Editor"}
          </button>
        </>
      ) : (
        <span className="grow small muted">Looking for GameX's recorder in the game…</span>
      )}
      {showKinds && view?.recorder && (
        <div className="redraw-kinds">
          {KINDS.filter((k) => counts[k.id] || !hidden.has(k.id)).map((k) => {
            const shown = !hidden.has(k.id);
            return (
              <label key={k.id} className={`redraw-kind kind-${k.id}${shown ? "" : " off"}`}>
                <input
                  type="checkbox"
                  checked={shown}
                  onChange={() => {
                    const next = new Set(hidden);
                    if (shown) next.add(k.id);
                    else next.delete(k.id);
                    saveHidden(next);
                  }}
                />
                <span className="pick-swatch" />
                <span className={shown ? undefined : "muted"}>{k.label}</span>
                <span className="count">{counts[k.id] ?? 0}</span>
                {!shown && <EyeOff size={12} className="muted" />}
              </label>
            );
          })}
          <span className="status-spacer" />
          <label className="small muted" title="How the arrow keys steer the player">
            Arrow keys:{" "}
            <select
              value={arrows}
              onChange={(e) => {
                const v = e.target.value as "tap" | "dpad";
                setArrows(v);
                store(arrowKey(projectId), v);
              }}
            >
              <option value="tap">tap next to the player</option>
              <option value="dpad">send D-pad keys</option>
            </select>
          </label>
        </div>
      )}
      {saved && (
        <div className="notice small live-captured redraw-kinds">
          <span className="grow">
            "{saved.name}": {saved.tiles} tiles, {saved.sprites} other pictures, {saved.objects} of the game's objects
            {saved.player ? `, ${saved.player.split(".").pop()} as the player` : ""}.
            {saved.missing ? ` ${saved.missing} picture${saved.missing === 1 ? " wasn't" : "s weren't"} fetched in time (shown as boxes).` : ""}
          </span>
          {onOpenEditor && (
            <button className="small primary" onClick={() => onOpenEditor(saved.scene)}>
              Open in Editor
            </button>
          )}
          <button className="link" onClick={() => setSaved(null)}>
            Dismiss
          </button>
        </div>
      )}
    </div>
  );

  return { boxes, pause: { paused, onToggle: togglePause }, onArrow, panel };
}
