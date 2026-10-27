import { Activity, Link2, Link2Off, Play, RefreshCw, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { categories } from "../lib/categories";
import type { Category, GameObject, LiveClassCount, LiveField, LiveInstance, LiveObject } from "../lib/types";
import { CategoryIcon } from "../lib/visuals";

interface Props {
  projectId: string;
  objects: GameObject[];
  /** Opens a class in the Content tab. */
  onOpenObject: (id: string) => void;
  onRun: () => void;
}

/**
 * Live inspector for a running Java/Kotlin game: the game's classes with how
 * many objects of each are alive, the objects themselves, and their fields,
 * which can be changed while the game runs.
 */
export default function LiveView({ projectId, objects, onOpenObject, onRun }: Props) {
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
      <div className="live-empty">
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
    );
  }

  return (
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
