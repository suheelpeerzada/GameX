import { Activity, Crosshair, Eye, EyeOff, Link2, Link2Off, Play, RefreshCw, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { categories } from "../lib/categories";
import { type Calibration, type CalibrationPoint, classGroups, nearest, solveCalibration, toScreen } from "../lib/picker";
import type { Category, GameObject, LiveClassCount, LiveField, LiveInstance, LiveObject, LivePosition, UiNode } from "../lib/types";
import { CategoryIcon } from "../lib/visuals";
import GameScreen, { type PickMarker, type PickMode } from "./GameScreen";

interface Calibrated {
  calibration: Calibration | null;
  points: CalibrationPoint[];
}

const calibrationKey = (projectId: string) => `gamex.pick.${projectId}`;

function loadCalibration(projectId: string): Calibrated {
  try {
    const saved = JSON.parse(localStorage.getItem(calibrationKey(projectId)) ?? "null");
    if (saved && typeof saved === "object") return { calibration: saved.calibration ?? null, points: [] };
  } catch {
    // Nothing saved, or storage is off.
  }
  return { calibration: null, points: [] };
}

const hiddenKey = (projectId: string) => `gamex.pick.hidden.${projectId}`;

function loadHidden(projectId: string): Set<string> {
  try {
    const saved = JSON.parse(localStorage.getItem(hiddenKey(projectId)) ?? "[]");
    if (Array.isArray(saved)) return new Set(saved.filter((c) => typeof c === "string"));
  } catch {
    // Nothing saved, or storage is off.
  }
  return new Set();
}

/** How close (device pixels) the pointer must be to an object to hit it. */
const PICK_RADIUS = 70;

interface Props {
  projectId: string;
  objects: GameObject[];
  /** Opens a class in the Content tab. */
  onOpenObject: (id: string) => void;
  onRun: () => void;
  /** Whether the Live tab is on screen. */
  visible: boolean;
}

/**
 * Live inspector for a running Java/Kotlin game: the game's classes with how
 * many objects of each are alive, the objects themselves, and their fields,
 * which can be changed while the game runs.
 */
export default function LiveView({ projectId, objects, onOpenObject, onRun, visible }: Props) {
  const [connected, setConnected] = useState<{ pid: number; package: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [counts, setCounts] = useState<Record<string, number | null>>({});
  const [onlyLive, setOnlyLive] = useState(true);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [selectedClass, setSelectedClass] = useState<string | null>(null);
  const [instances, setInstances] = useState<LiveInstance[]>([]);
  const [selectedObject, setSelectedObject] = useState<string | null>(null);
  const [object, setObject] = useState<LiveObject | null>(null);
  const [filter, setFilter] = useState("");
  const [showStatic, setShowStatic] = useState(false);

  // The picker: hover/click on the game's screen to find what's there.
  const [picking, setPicking] = useState(false);
  const [positions, setPositions] = useState<LivePosition[]>([]);
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);
  const [calibrated, setCalibrated] = useState<Calibrated>(() => loadCalibration(projectId));
  const [calibrating, setCalibrating] = useState(false);
  const [uiHit, setUiHit] = useState<UiNode | null>(null);
  const [pickNote, setPickNote] = useState<string | null>(null);
  // Classes whose objects aren't drawn over the screen (chosen by the user).
  const [hidden, setHidden] = useState<Set<string>>(() => loadHidden(projectId));
  const [showClasses, setShowClasses] = useState(false);
  const positionsBusy = useRef(false);

  // The game's own classes (not libraries), grouped by category.
  const classes = useMemo(
    () => objects.filter((o) => o.id.startsWith("class:") && o.kind === "class"),
    [objects],
  );
  const classNames = useMemo(() => classes.map((c) => c.id.slice("class:".length)), [classes]);

  const fail = (e: unknown) => {
    const message = String(e);
    setError(message);
    if (message.includes("connection to the game was lost") || message.includes("Not connected")) setConnected(null);
  };

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      const info = await api.liveAttach(projectId);
      setConnected({ pid: info.pid, package: info.package });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = () => {
    api.liveDetach(projectId);
    setConnected(null);
    setInstances([]);
    setObject(null);
    setPositions([]);
  };

  const refreshCounts = useCallback(async () => {
    try {
      const list: LiveClassCount[] = await api.liveCounts(projectId, classNames);
      setCounts(Object.fromEntries(list.map((c) => [c.class, c.count])));
    } catch (e) {
      fail(e);
    }
  }, [projectId, classNames]);

  const refreshInstances = useCallback(async () => {
    if (!selectedClass) return;
    try {
      setInstances(await api.liveInstances(projectId, selectedClass));
    } catch (e) {
      fail(e);
    }
  }, [projectId, selectedClass]);

  const refreshObject = useCallback(async () => {
    if (!selectedObject) return;
    try {
      setObject(await api.liveObject(projectId, selectedObject));
    } catch (e) {
      const message = String(e);
      if (message.includes("no longer exists")) {
        setObject(null);
        setSelectedObject(null);
        setError("That object no longer exists: the game removed it.");
      } else fail(e);
    }
  }, [projectId, selectedObject]);

  useEffect(() => {
    if (connected) refreshCounts();
  }, [connected, refreshCounts]);
  useEffect(() => {
    if (connected) refreshInstances();
  }, [connected, refreshInstances]);
  useEffect(() => {
    if (connected) refreshObject();
  }, [connected, refreshObject]);

  // Live refresh: counts and the open object's values.
  useEffect(() => {
    if (!connected || !autoRefresh) return;
    const timer = window.setInterval(() => {
      refreshCounts();
      refreshObject();
    }, 1500);
    return () => window.clearInterval(timer);
  }, [connected, autoRefresh, refreshCounts, refreshObject]);

  // While picking, where the live objects are (about once a second). Setup
  // and helper classes (Core, Code) are left out: they aren't on screen.
  const liveClasses = useMemo(
    () =>
      classes
        .filter((c) => c.category !== "core" && c.category !== "code")
        .map((c) => c.id.slice("class:".length))
        .filter((c) => (counts[c] ?? 0) > 0),
    [classes, counts],
  );
  useEffect(() => {
    if (!connected || !picking || !visible) return;
    const tick = async () => {
      if (positionsBusy.current) return;
      positionsBusy.current = true;
      try {
        setPositions(await api.livePositions(projectId, liveClasses));
      } catch (e) {
        fail(e);
      } finally {
        positionsBusy.current = false;
      }
    };
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connected, picking, visible, projectId, liveClasses]);

  const saveCalibration = (next: Calibrated) => {
    setCalibrated(next);
    try {
      localStorage.setItem(calibrationKey(projectId), JSON.stringify({ calibration: next.calibration }));
    } catch {
      // Not remembered across restarts; still works now.
    }
  };

  const saveHidden = (next: Set<string>) => {
    setHidden(next);
    try {
      localStorage.setItem(hiddenKey(projectId), JSON.stringify([...next]));
    } catch {
      // Not remembered across restarts; still works now.
    }
  };

  // An object at exactly 0,0 is almost always one not placed yet.
  const placed = useMemo(() => positions.filter((p) => p.x !== 0 || p.y !== 0), [positions]);
  // The classes with objects that could be drawn, and how many of each.
  const placedClasses = useMemo(() => classGroups(placed), [placed]);

  const calibration = calibrated.calibration;
  const markers: (PickMarker & { position: LivePosition })[] = useMemo(() => {
    if (!calibration) return [];
    const colors = new Map(placedClasses.map((c) => [c.name, c.color]));
    return placed
      .filter((p) => !hidden.has(p.class))
      .map((p) => ({
        ...toScreen(calibration, p.x, p.y),
        key: p.object,
        label: `${p.class.split(".").pop()} #${p.object}`,
        color: colors.get(p.class),
        position: p,
      }));
  }, [placed, placedClasses, hidden, calibration]);
  const hovered = pointer && !calibrating ? nearest(markers, pointer, PICK_RADIUS) : -1;
  const selectedPosition = positions.find((p) => p.object === selectedObject);

  const startCalibrating = () => {
    setCalibrating(true);
    setCalibrated((c) => ({ ...c, points: [] }));
    setUiHit(null);
    setPickNote(null);
  };

  const onPick = async (p: { x: number; y: number }) => {
    setPickNote(null);
    // Lining up: asked for, or the first click with a placed object selected.
    if (connected && (calibrating || (!calibration && selectedPosition))) {
      if (!selectedPosition) {
        setPickNote(
          selectedObject
            ? "This object has no position fields GameX recognizes (like x/y). Pick another object, one you can see on screen."
            : "First pick an object in the Objects list (one you can see on screen), then click where it is on the game.",
        );
        return;
      }
      const points = [...calibrated.points, { wx: selectedPosition.x, wy: selectedPosition.y, sx: p.x, sy: p.y }].slice(-6);
      const solved = solveCalibration(points, calibration);
      saveCalibration({ calibration: solved ?? calibration, points });
      if (solved && (points.length >= 2 || calibration)) {
        setCalibrating(false);
        setPickNote("Lined up. Hover over the game to find its objects; click one to inspect it.");
      } else {
        setPickNote("Got it. Now pick a different object, somewhere else on screen, and click where it is.");
      }
      return;
    }
    if (connected && hovered >= 0) {
      const hit = markers[hovered].position;
      setUiHit(null);
      setSelectedClass(hit.class);
      setSelectedObject(hit.object);
      return;
    }
    try {
      const node = await api.uiPick(projectId, p.x, p.y);
      setUiHit(node);
      if (!node) setPickNote("Nothing on screen there.");
      else if (node.surface)
        setPickNote(
          connected
            ? "No game object here (or its position isn't known). The game draws this area itself."
            : "The game draws this area itself. Connect to the running game to find the objects in it.",
        );
    } catch (e) {
      setPickNote(String(e));
    }
  };

  const pick: PickMode = {
    active: picking,
    markers: markers.map((m, i) => ({
      key: m.key,
      x: m.x,
      y: m.y,
      label: m.label,
      color: m.color,
      state: i === hovered ? "hover" : m.key === selectedObject ? "selected" : undefined,
    })),
    box: uiHit && !uiHit.surface ? uiHit.bounds : null,
    onToggle: () => {
      setPicking((on) => !on);
      setPointer(null);
    },
    onMove: setPointer,
    onPick,
  };

  const layoutObject = uiHit?.layout
    ? objects.find((o) => o.sourceFiles.some((f) => f === uiHit.layout || uiHit.layout!.endsWith(f) || f.endsWith(uiHit.layout!.replace(/^apktool\//, ""))))
    : undefined;

  const pickPanel = picking && (
    <div className="pick-bar">
      <Crosshair size={15} />
      <div className="grow small">
        {pickNote ??
          (connected && calibrating
            ? "Line up the game's coordinates with the screen: pick an object you can see in the Objects list, then click where it is on the game. Do this for two objects in different spots."
            : connected && !calibration
              ? "Click a button, text or image to see which view it is. To point at the game's own objects (characters, items...), line them up with the screen first."
              : connected
              ? `Hover over the game to find its objects (${markers.length} with a known position); click one to inspect it. Buttons and other views can be clicked too.`
              : "Click a button, text or image on the game to see which view it is and where it's declared.")}
        {uiHit && (
          <div className="pick-hit">
            <span className="muted">View</span>
            <span className="mono">{uiHit.class.split(".").pop()}</span>
            {uiHit.resourceId && (
              <>
                <span className="muted">ID</span>
                <span className="mono">{uiHit.resourceId.split("/").pop()}</span>
              </>
            )}
            {(uiHit.text || uiHit.description) && (
              <>
                <span className="muted">Text</span>
                <span>{uiHit.text || uiHit.description}</span>
              </>
            )}
            {!uiHit.surface && (
              <>
                <span className="muted">Declared in</span>
                <span>
                  {layoutObject ? (
                    <button className="link" onClick={() => onOpenObject(layoutObject.id)}>
                      {uiHit.layout}
                    </button>
                  ) : (
                    <span className="muted">{uiHit.layout ?? "made in code, not in a layout file"}</span>
                  )}
                </span>
              </>
            )}
          </div>
        )}
      </div>
      {connected && calibration && placedClasses.length > 0 && (
        <button
          className={showClasses ? "small active" : "small"}
          onClick={() => setShowClasses((s) => !s)}
          title="Choose which kinds of objects are marked on the screen"
        >
          <Eye size={13} /> Shown ({placedClasses.filter((c) => !hidden.has(c.name)).length}/{placedClasses.length})
        </button>
      )}
      {connected && !calibration && !calibrating && (
        <button className="small" onClick={startCalibrating}>
          Line up objects
        </button>
      )}
      {connected && calibration && !calibrating && (
        <button className="small" onClick={startCalibrating} title="Use this if the markers don't sit on the objects (for example after the camera moved)">
          Line up again
        </button>
      )}
      {connected && calibrating && (
        <button className="small" onClick={() => { setCalibrating(false); setPickNote(null); }}>
          Cancel
        </button>
      )}
      {connected && calibration && showClasses && placedClasses.length > 0 && (
        <div className="pick-classes">
          <div className="row small">
            <span className="muted">Mark on screen:</span>
            <button className="link" onClick={() => saveHidden(new Set())}>
              all
            </button>
            <button className="link" onClick={() => saveHidden(new Set(placedClasses.map((c) => c.name)))}>
              none
            </button>
          </div>
          {placedClasses.map((c) => {
            const shown = !hidden.has(c.name);
            return (
              <label key={c.name} className={shown ? "pick-class" : "pick-class off"} title={c.name}>
                <input
                  type="checkbox"
                  checked={shown}
                  onChange={() => {
                    const next = new Set(hidden);
                    if (shown) next.add(c.name);
                    else next.delete(c.name);
                    saveHidden(next);
                  }}
                />
                <span className="pick-swatch" style={{ background: c.color }} />
                {shown ? <span>{c.name.split(".").pop()}</span> : <span className="muted">{c.name.split(".").pop()}</span>}
                <span className="count">{c.count}</span>
                {!shown && <EyeOff size={12} className="muted" />}
              </label>
            );
          })}
        </div>
      )}
    </div>
  );

  const byCategory = useMemo(() => {
    const groups = new Map<Category, GameObject[]>();
    for (const c of classes) {
      const name = c.id.slice("class:".length);
      if (onlyLive && !(counts[name] && counts[name]! > 0)) continue;
      groups.set(c.category, [...(groups.get(c.category) ?? []), c]);
    }
    return groups;
  }, [classes, counts, onlyLive]);

  const totalLive = Object.values(counts).reduce<number>((n, c) => n + (c ?? 0), 0);
  const fields = (object?.fields ?? []).filter(
    (f) => (showStatic || !f.isStatic) && (!filter || f.name.toLowerCase().includes(filter.toLowerCase())),
  );

  if (!connected) {
    return (
      <div className="live-split">
        <GameScreen projectId={projectId} visible={visible} pick={pick} />
      <div className="live-empty">
        {pickPanel}
        <Activity size={28} className="ai-mark" />
        <h2>Live editing</h2>
        <p className="muted">
          See the objects inside the running game (characters, enemies, items…) and change their values while you play.
          Works for Java/Kotlin games. Start the game with Run, then connect.
        </p>
        {error && <div className="panel error small">{error}</div>}
        <div className="row">
          <button onClick={onRun}>
            <Play size={14} /> Run the game
          </button>
          <button className="primary" onClick={connect} disabled={busy}>
            <Link2 size={14} /> {busy ? "Connecting…" : "Connect to the running game"}
          </button>
        </div>
      </div>
      </div>
    );
  }

  return (
    <div className="live-split">
      <GameScreen projectId={projectId} visible={visible} pick={pick} />
    <div className="live">
      <div className="live-bar">
        <span className="status-dot ok" />
        <span>
          Connected to <span className="mono small">{connected.package}</span>{" "}
          <span className="muted small">(process {connected.pid})</span>
        </span>
        <span className="muted small">· {totalLive.toLocaleString()} live objects in the game's classes</span>
        <span className="status-spacer" />
        <label className="small">
          <input type="checkbox" checked={autoRefresh} onChange={(e) => setAutoRefresh(e.target.checked)} /> Live refresh
        </label>
        <button onClick={() => { refreshCounts(); refreshInstances(); refreshObject(); }} title="Refresh now">
          <RefreshCw size={14} />
        </button>
        <button onClick={disconnect}>
          <Link2Off size={14} /> Disconnect
        </button>
      </div>
      {error && (
        <div className="panel error small live-error">
          {error} <button className="link" onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}

      {pickPanel}
      <div className="live-columns">
        <section className="live-col">
          <div className="live-col-head">
            <strong>Classes</strong>
            <label className="small muted">
              <input type="checkbox" checked={onlyLive} onChange={(e) => setOnlyLive(e.target.checked)} /> only live
            </label>
          </div>
          <div className="live-scroll">
            {categories
              .filter((c) => byCategory.has(c.id))
              .map((cat) => (
                <div key={cat.id} className="live-group">
                  <div className="live-group-head">
                    <CategoryIcon category={cat.id} size={13} /> {cat.label}
                  </div>
                  {byCategory.get(cat.id)!.map((c) => {
                    const name = c.id.slice("class:".length);
                    const count = counts[name];
                    return (
                      <button
                        key={c.id}
                        className={selectedClass === name ? "live-row selected" : "live-row"}
                        onClick={() => {
                          setSelectedClass(name);
                          setSelectedObject(null);
                          setObject(null);
                        }}
                      >
                        <span className="tree-name">{c.displayName}</span>
                        {c.role && <span className="tag">{c.role}</span>}
                        <span className="count">{count === null || count === undefined ? "–" : count}</span>
                      </button>
                    );
                  })}
                </div>
              ))}
            {byCategory.size === 0 && (
              <p className="muted small">
                {onlyLive ? "No objects of the game's classes are alive right now. Play into a level, or untick “only live”." : "No game classes."}
              </p>
            )}
          </div>
        </section>

        <section className="live-col">
          <div className="live-col-head">
            <strong>{selectedClass ? selectedClass.split(".").pop() : "Objects"}</strong>
            {selectedClass && (
              <button className="link small" onClick={() => onOpenObject(`class:${selectedClass}`)}>
                Open source
              </button>
            )}
          </div>
          <div className="live-scroll">
            {!selectedClass && <p className="muted small">Pick a class to see its live objects.</p>}
            {selectedClass && instances.length === 0 && <p className="muted small">No live objects of this class.</p>}
            {instances.map((i) => (
              <button
                key={i.id}
                className={selectedObject === i.id ? "live-row selected" : "live-row"}
                onClick={() => setSelectedObject(i.id)}
              >
                <span className="mono small">#{i.id}</span>
                <span className="muted small tree-name">{i.summary}</span>
              </button>
            ))}
          </div>
        </section>

        <section className="live-col wide">
          <div className="live-col-head">
            <strong>{object ? `${object.class.split(".").pop()} #${object.id}` : "Fields"}</strong>
            {object && (
              <>
                <label className="search-box live-filter">
                  <Search size={13} />
                  <input type="text" placeholder="Filter fields" value={filter} onChange={(e) => setFilter(e.target.value)} />
                </label>
                <label className="small muted">
                  <input type="checkbox" checked={showStatic} onChange={(e) => setShowStatic(e.target.checked)} /> static
                </label>
              </>
            )}
          </div>
          <div className="live-scroll">
            {!object && <p className="muted small">Pick an object to see and change its values.</p>}
            {object && (
              <table className="live-fields">
                <tbody>
                  {fields.map((f) => (
                    <FieldRow
                      key={f.fieldId}
                      field={f}
                      onSet={async (value) => {
                        try {
                          const shown = await api.liveSet(projectId, object.id, f.fieldId, value);
                          setObject((o) => o && { ...o, fields: o.fields.map((x) => (x.fieldId === f.fieldId ? { ...x, value: shown } : x)) });
                          setError(null);
                          return true;
                        } catch (e) {
                          setError(String(e));
                          return false;
                        }
                      }}
                    />
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </section>
      </div>
      <p className="muted tiny live-note">
        Changes apply to the running game immediately and last until the game restarts. To keep a change, edit the
        class's smali (Open source) and rebuild.
      </p>
    </div>
    </div>
  );
}

function FieldRow({ field, onSet }: { field: LiveField; onSet: (value: string) => Promise<boolean> }) {
  const [draft, setDraft] = useState<string | null>(null);
  const editing = draft !== null;
  const apply = async () => {
    if (draft === null) return;
    const ok = await onSet(draft.replace(/^"|"$/g, ""));
    if (ok) setDraft(null);
  };
  return (
    <tr className={field.isStatic ? "static" : undefined}>
      <td className="live-field-name" title={`declared in ${field.declaredIn}`}>
        {field.name}
        <span className="muted tiny"> {field.typeName}</span>
      </td>
      <td>
        {field.editable ? (
          <input
            type="text"
            className={editing ? "mono live-input editing" : "mono live-input"}
            value={editing ? draft : field.value}
            onFocus={() => setDraft(field.value)}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => setDraft(null)}
            onKeyDown={(e) => {
              if (e.key === "Enter") apply();
              if (e.key === "Escape") (e.target as HTMLInputElement).blur();
            }}
            title="Type a new value and press Enter"
          />
        ) : (
          <span className="mono small muted">{field.value}</span>
        )}
      </td>
    </tr>
  );
}
