import { ChevronDown, ChevronRight, Copy, Image as ImageIcon, Plus, Replace, RotateCcw, Trash2, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import type { Behavior, Entity, Part, Property, Thing, ThingKind, Transform, Value, Vec3 } from "../../lib/gx";
import {
  effectiveProperties,
  eulerToQuat,
  parseValue,
  quatToEuler,
  ruleText,
  transformOf,
  valueOf,
  valueText,
  words,
} from "../../lib/gxscene";

/** A text field that changes nothing until Enter or leaving it; Escape puts it back. */
function Field({
  value,
  onCommit,
  className,
  title,
}: {
  value: string;
  /** Returns an error message to keep the field open, or nothing when accepted. */
  onCommit: (text: string) => string | void;
  className?: string;
  title?: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setError(null), [value]);
  const commit = () => {
    if (draft === null || draft === value) return setDraft(null);
    const problem = onCommit(draft);
    if (problem) setError(problem);
    else {
      setError(null);
      setDraft(null);
    }
  };
  return (
    <input
      type="text"
      className={`${className ?? ""}${error ? " invalid" : ""}`}
      value={draft ?? value}
      title={error ?? title}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") {
          setDraft(null);
          setError(null);
        }
      }}
    />
  );
}

const num = (v: number) => String(Number(v.toFixed(4)));

/** Three numbers in a row: x, y, z. */
function Vec3Field({ value, onCommit, step }: { value: Vec3; onCommit: (v: Vec3) => void; step?: string }) {
  return (
    <div className="gx-vec3">
      {(["x", "y", "z"] as const).map((axis, i) => (
        <label key={axis} className={`gx-axis gx-axis-${axis}`} title={step}>
          <span>{axis}</span>
          <Field
            value={num(value[i])}
            onCommit={(text) => {
              const n = Number(text);
              if (text.trim() === "" || !Number.isFinite(n)) return "Type a number.";
              const next = [...value] as Vec3;
              next[i] = n;
              onCommit(next);
            }}
          />
        </label>
      ))}
    </div>
  );
}

function TransformSection({ transform, onCommit }: { transform: Transform; onCommit: (t: Transform, summary: string) => void }) {
  return (
    <section className="gx-section">
      <h4>Transform</h4>
      <div className="gx-row">
        <span className="gx-label">Position</span>
        <Vec3Field value={transform.position} step="metres" onCommit={(position) => onCommit({ ...transform, position }, "Move")} />
      </div>
      <div className="gx-row">
        <span className="gx-label">Rotation</span>
        <Vec3Field
          value={quatToEuler(transform.rotation)}
          step="degrees"
          onCommit={(deg) => onCommit({ ...transform, rotation: eulerToQuat(deg) }, "Turn")}
        />
      </div>
      <div className="gx-row">
        <span className="gx-label">Scale</span>
        <Vec3Field value={transform.scale} onCommit={(scale) => onCommit({ ...transform, scale }, "Resize")} />
      </div>
    </section>
  );
}

const PART_NAMES: Record<Part["type"], string> = {
  shape: "Shape",
  model: "3D model",
  sprite: "Sprite",
  collider: "Collider",
  light: "Light",
  camera: "Camera",
  sound: "Sound",
  text: "Text",
  tiles: "Tile map",
  custom: "Engine component",
};

function partSummary(p: Part): string {
  switch (p.type) {
    case "shape":
      return `${p.shape}, ${p.size.map(num).join(" × ")} m`;
    case "collider":
      return `${p.shape}, ${p.body}${p.trigger ? ", trigger" : ""}`;
    case "light":
      return `${p.kind}, intensity ${p.intensity}${p.range ? `, range ${p.range} m` : ""}`;
    case "camera":
      return p.orthoSize ? `2D, ${p.orthoSize} m tall` : `${p.fov ?? 60}° view`;
    case "model":
    case "sprite":
    case "sound":
      return p.asset;
    case "text":
      return `"${p.text}"`;
    case "tiles":
      return `${p.cells.length} tiles from ${p.sheets.length} sheet${p.sheets.length === 1 ? "" : "s"}`;
    case "custom":
      return `${p.name} (kept from the original game)`;
  }
}

/** A part, with its colour and size editable where it has them. */
function PartRow({ part, onChange }: { part: Part; onChange?: (p: Part, summary: string) => void }) {
  return (
    <li className="gx-part">
      <div className="gx-part-head">
        <strong>{PART_NAMES[part.type]}</strong>
        <span className="muted small">{partSummary(part)}</span>
      </div>
      {onChange && (part.type === "shape" || part.type === "light") && (
        <div className="gx-row">
          <span className="gx-label">Colour</span>
          <input
            type="color"
            value={part.color.slice(0, 7)}
            onChange={(e) => onChange({ ...part, color: e.target.value }, "Recolour")}
          />
          <span className="mono small">{part.color}</span>
        </div>
      )}
      {onChange && part.type === "shape" && (
        <div className="gx-row">
          <span className="gx-label">Size</span>
          <Vec3Field value={part.size} step="metres" onCommit={(size) => onChange({ ...part, size }, "Resize")} />
        </div>
      )}
      {onChange && part.type === "light" && (
        <div className="gx-row">
          <span className="gx-label">Intensity</span>
          <Field
            value={num(part.intensity)}
            onCommit={(t) => {
              const n = Number(t);
              if (!Number.isFinite(n) || n < 0) return "Type a number, 0 or more.";
              onChange({ ...part, intensity: n }, "Change light");
            }}
          />
        </div>
      )}
    </li>
  );
}

function ValueField({ value, onCommit }: { value: Value; onCommit: (v: Value) => void }) {
  if (value.type === "bool") {
    return <input type="checkbox" checked={value.value} onChange={(e) => onCommit({ type: "bool", value: e.target.checked })} />;
  }
  if (value.type === "color") {
    return <input type="color" value={value.value.slice(0, 7)} onChange={(e) => onCommit({ type: "color", value: e.target.value })} />;
  }
  return (
    <Field
      value={valueText(value)}
      onCommit={(text) => {
        const parsed = parseValue(value, text);
        if ("error" in parsed) return parsed.error;
        onCommit(parsed);
      }}
    />
  );
}

function BehaviorList({
  uses,
  behaviors,
  from,
}: {
  uses: { behavior: string }[];
  behaviors: Record<string, Behavior | null>;
  from?: string;
}) {
  if (uses.length === 0) return null;
  return (
    <section className="gx-section">
      <h4>Behavior{from ? <span className="muted small"> from {from}</span> : null}</h4>
      {uses.map((u) => (
        <BehaviorView key={u.behavior} id={u.behavior} behavior={behaviors[u.behavior]} />
      ))}
    </section>
  );
}

/** A behavior's rules, read as sentences. */
export function BehaviorView({ id, behavior }: { id: string; behavior: Behavior | null | undefined }) {
  if (behavior === undefined) return <p className="muted small">Loading {id}…</p>;
  if (behavior === null) return <p className="panel error small">The behavior "{id}" isn't in this project.</p>;
  return (
    <div className="gx-behavior">
      <div className="gx-part-head">
        <strong>{behavior.name}</strong>
        {behavior.states?.length ? <span className="muted small">states: {behavior.states.join(", ")}</span> : null}
      </div>
      {behavior.description && <p className="muted small">{behavior.description}</p>}
      <ol className="gx-rules">
        {behavior.rules.map((r, i) => (
          <li key={i}>{ruleText(r)}</li>
        ))}
      </ol>
      {behavior.params?.length ? (
        <p className="muted small">
          Settings: {behavior.params.map((p) => `${words(p.name)} ${valueText(valueOf(p))}${p.unit ? ` ${p.unit}` : ""}`).join(", ")}
        </p>
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Pieces both inspectors use (laid out like the mockup)

/** What an asset looks like: its picture (images), or an icon. */
function Thumb({ asset, readAsset, size = 48 }: { asset?: string; readAsset?: (asset: string) => Promise<ArrayBuffer>; size?: number }) {
  const [url, setUrl] = useState<string | null>(null);
  const [px, setPx] = useState<string | null>(null);
  useEffect(() => {
    setUrl(null);
    if (!asset || !readAsset || !/\.(png|jpe?g|webp|gif|bmp|svg)$/i.test(asset)) return;
    let made: string | null = null;
    let cancelled = false;
    readAsset(asset)
      .then((bytes) => {
        if (cancelled) return;
        made = URL.createObjectURL(new Blob([bytes], { type: asset.endsWith(".svg") ? "image/svg+xml" : "" }));
        setUrl(made);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      if (made) URL.revokeObjectURL(made);
    };
  }, [asset, readAsset]);
  return (
    <span className="gx-thumb" style={{ width: size, height: size }} title={px ?? undefined}>
      {url ? <img src={url} alt="" onLoad={(e) => setPx(`${e.currentTarget.naturalWidth} × ${e.currentTarget.naturalHeight}`)} /> : <ImageIcon size={size * 0.45} className="muted" />}
    </span>
  );
}

/** The asset a thing or entity is drawn with: its sprite or model. */
function lookOf(parts: Part[] | undefined): string | undefined {
  const p = parts?.find((x) => x.type === "sprite" || x.type === "model");
  return p && "asset" in p ? p.asset : undefined;
}

/** A fold-away part of the inspector. */
function Fold({ title, open: initiallyOpen = true, children }: { title: string; open?: boolean; children: ReactNode }) {
  const [open, setOpen] = useState(initiallyOpen);
  return (
    <section className="gx-section gx-fold">
      <button className="gx-fold-head" onClick={() => setOpen((o) => !o)}>
        {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />} {title}
      </button>
      {open && children}
    </section>
  );
}

function LookSection({ asset, readAsset, onReplace }: { asset?: string; readAsset?: (asset: string) => Promise<ArrayBuffer>; onReplace?: (asset: string) => void }) {
  if (!asset) return null;
  return (
    <Fold title="Sprite / Model">
      <div className="gx-look">
        <Thumb asset={asset} readAsset={readAsset} size={56} />
        <span className="grow small gx-look-name" title={asset}>
          {asset.split("/").pop()}
        </span>
        {onReplace && (
          <button className="small" onClick={() => onReplace(asset)}>
            <Replace size={13} /> Replace
          </button>
        )}
      </div>
    </Fold>
  );
}

const SWITCHES = [
  { tag: "no-move", label: "Can Move" },
  { tag: "no-attack", label: "Can Attack" },
  { tag: "no-collide", label: "Collides" },
];

/** Can Move / Can Attack / Collides: on unless switched off (a "no-..." tag), read by the GameX engine. */
function SwitchesSection({ tags, inherited, onChange }: { tags: string[]; inherited?: string[]; onChange: (tags: string[], summary: string) => void }) {
  return (
    <Fold title="Behavior">
      {SWITCHES.map(({ tag, label }) => {
        const off = tags.includes(tag) || !!inherited?.includes(tag);
        const fromThing = !tags.includes(tag) && !!inherited?.includes(tag);
        return (
          <label key={tag} className="gx-switch-row" title={fromThing ? "Switched off on the thing, for all of them" : undefined}>
            <span className="grow">{label}</span>
            <input
              type="checkbox"
              className="gx-switch"
              checked={!off}
              disabled={fromThing}
              onChange={(e) => onChange(e.target.checked ? tags.filter((t) => t !== tag) : [...tags, tag], `${e.target.checked ? "Let" : "Stop"} ${label.toLowerCase().replace("can ", "")}:`)}
            />
          </label>
        );
      })}
    </Fold>
  );
}

/** Tags as chips; the Behavior switches' own tags aren't shown here. */
function TagsSection({ tags, onChange }: { tags: string[]; onChange: (tags: string[], summary: string) => void }) {
  const [adding, setAdding] = useState(false);
  const shown = tags.filter((t) => !SWITCHES.some((s) => s.tag === t));
  return (
    <Fold title="Tags">
      <div className="gx-chips">
        {shown.map((t) => (
          <span key={t} className="gx-chip">
            {t}
            <button title={`Remove ${t}`} onClick={() => onChange(tags.filter((x) => x !== t), `Untag ${t}:`)}>
              <X size={10} />
            </button>
          </span>
        ))}
        {adding ? (
          <Field
            className="gx-chip-input"
            value=""
            onCommit={(text) => {
              const t = text.trim();
              setAdding(false);
              if (t && !tags.includes(t)) onChange([...tags, t], `Tag ${t}:`);
            }}
          />
        ) : (
          <button className="gx-chip add" onClick={() => setAdding(true)} title="Add a tag">
            <Plus size={11} />
          </button>
        )}
      </div>
    </Fold>
  );
}

/** A sensible range to show beside a number, from its name (as the mockup's "0 – 999"). */
function rangeOf(name: string): string | null {
  const n = name.toLowerCase();
  if (n.includes("speed")) return "0.1 – 20";
  if (/health|hp|damage|attack|score|points|value|lives/.test(n)) return "0 – 999";
  if (/jump|height|range|distance|radius/.test(n)) return "0 – 50";
  return null;
}

interface EntityProps {
  entity: Entity;
  thing?: Thing;
  behaviors: Record<string, Behavior | null>;
  onChange: (next: Entity, summary: string) => void;
  onOpenThing: (id: string) => void;
  onDuplicate: () => void;
  onDelete: () => void;
  readAsset?: (asset: string) => Promise<ArrayBuffer>;
  onReplace?: (asset: string) => void;
}

/** The selected entity: name, transform, parts, properties and behavior. */
export function EntityInspector({ entity, thing, behaviors, onChange, onOpenThing, onDuplicate, onDelete, readAsset, onReplace }: EntityProps) {
  const look = lookOf(entity.parts) ?? lookOf(thing?.parts);
  const transform = transformOf(entity);
  const props = effectiveProperties(thing, entity);
  const change = (patch: Partial<Entity>, summary: string) => onChange({ ...entity, ...patch }, `${summary} ${entity.name}`);
  const setOverride = (name: string, value: Value | null) => {
    const overrides = { ...(entity.overrides ?? {}) };
    if (value) overrides[name] = value;
    else delete overrides[name];
    change({ overrides: Object.keys(overrides).length ? overrides : undefined }, value ? `Set ${words(name)} on` : `Reset ${words(name)} on`);
  };
  return (
    <div className="gx-inspector">
      <div className="gx-head">
        <Thumb asset={look} readAsset={readAsset} />
        <div className="grow gx-head-text">
          <Field className="gx-name" value={entity.name} onCommit={(name) => (name.trim() ? change({ name: name.trim() }, "Rename") : "Give it a name.")} />
          <span className="gx-pill">{thing ? thing.kind[0].toUpperCase() + thing.kind.slice(1) : "Object"}</span>
        </div>
      </div>
      <div className="gx-title-row">
        <label className="small" title="Inactive things are hidden and don't act">
          <input type="checkbox" checked={entity.active !== false} onChange={(e) => change({ active: e.target.checked ? undefined : false }, e.target.checked ? "Show" : "Hide")} />{" "}
          active
        </label>
      </div>
      <div className="muted small gx-id">
        {entity.id}
        {entity.thing && (
          <>
            {" · "}
            {thing ? (
              <button className="link" onClick={() => onOpenThing(thing.id)}>
                a {thing.name}
                {thing.role ? ` (${thing.role})` : ""}
              </button>
            ) : (
              <span className="bad">a missing thing "{entity.thing}"</span>
            )}
          </>
        )}
      </div>
      <div className="row gx-actions">
        <button className="small" onClick={onDuplicate} title="Duplicate (Ctrl+D)">
          <Copy size={13} /> Duplicate
        </button>
        <button className="small" onClick={onDelete} title="Delete (Del)">
          <Trash2 size={13} /> Delete
        </button>
      </div>

      <TransformSection transform={transform} onCommit={(t, summary) => change({ transform: t }, summary)} />

      {props.length > 0 && (
        <section className="gx-section">
          <h4>Properties</h4>
          {props.map((p) => (
            <div key={p.name} className={p.overridden ? "gx-row gx-prop overridden" : "gx-row gx-prop"} title={p.description}>
              <span className="gx-label">{words(p.name)}</span>
              <ValueField value={p.value} onCommit={(v) => setOverride(p.name, v)} />
              {p.unit && <span className="muted small">{p.unit}</span>}
              {(p.value.type === "number" || p.value.type === "integer") && rangeOf(p.name) && <span className="muted tiny gx-range">{rangeOf(p.name)}</span>}
              {p.overridden && (
                <button
                  className="icon-button gx-reset"
                  title={p.base ? `Back to the ${thing?.name ?? "thing"}'s value (${valueText(p.base)})` : "Remove this value"}
                  onClick={() => setOverride(p.name, null)}
                >
                  <RotateCcw size={12} />
                </button>
              )}
            </div>
          ))}
          {thing && <p className="muted tiny">Changing a value here changes this one only. Change it on the {thing.name} to change all of them.</p>}
        </section>
      )}

      <LookSection asset={look} readAsset={readAsset} onReplace={onReplace} />
      <SwitchesSection tags={entity.tags ?? []} inherited={thing?.tags} onChange={(tags, summary) => change({ tags: tags.length ? tags : undefined }, summary)} />
      <TagsSection tags={entity.tags ?? []} onChange={(tags, summary) => change({ tags: tags.length ? tags : undefined }, summary)} />

      <Fold title="Advanced" open={false}>
      {thing?.parts?.length || entity.parts?.length ? (
        <section className="gx-section">
          <h4>Made of</h4>
          <ul className="gx-parts">
            {(entity.parts ?? []).map((p, i) => (
              <PartRow
                key={`own-${i}`}
                part={p}
                onChange={(next, summary) => change({ parts: entity.parts!.map((q, j) => (j === i ? next : q)) }, summary)}
              />
            ))}
            {(thing?.parts ?? []).map((p, i) => (
              <PartRow key={`thing-${i}`} part={p} />
            ))}
          </ul>
          {thing?.parts?.length ? <p className="muted tiny">Parts from the {thing.name} are changed on the {thing.name}.</p> : null}
        </section>
      ) : null}

      <BehaviorList uses={entity.behaviors ?? []} behaviors={behaviors} />
      <BehaviorList uses={thing?.behaviors ?? []} behaviors={behaviors} from={thing?.name} />
      </Fold>
    </div>
  );
}

/** Several entities selected: what can be done to all of them at once. */
export function MultiInspector({
  entities,
  onGroup,
  onDuplicate,
  onDelete,
}: {
  entities: Entity[];
  onGroup: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="gx-inspector">
      <div className="gx-name">{entities.length} selected</div>
      <p className="muted small">
        {entities
          .slice(0, 8)
          .map((e) => e.name)
          .join(", ")}
        {entities.length > 8 ? ` and ${entities.length - 8} more` : ""}
      </p>
      <div className="row gx-actions">
        <button className="small" onClick={onGroup} title="Put them in a new group (Ctrl+G)">
          <Plus size={13} /> Group
        </button>
        <button className="small" onClick={onDuplicate} title="Duplicate (Ctrl+D)">
          <Copy size={13} /> Duplicate
        </button>
        <button className="small" onClick={onDelete} title="Delete (Del)">
          <Trash2 size={13} /> Delete
        </button>
      </div>
      <p className="muted tiny">The handles in the view move, turn and resize them together, around their middle.</p>
    </div>
  );
}

const KINDS: ThingKind[] =["character", "item", "tool", "prop", "other"];
const ROLES = ["", "player", "enemy", "npc", "ally"];

interface ThingProps {
  thing: Thing;
  instances: number;
  behaviors: Record<string, Behavior | null>;
  canPlace: boolean;
  onChange: (next: Thing, summary: string) => void;
  onPlace: () => void;
  readAsset?: (asset: string) => Promise<ArrayBuffer>;
  onReplace?: (asset: string) => void;
}

/** A thing: what every instance of it starts with. */
export function ThingInspector({ thing, instances, behaviors, canPlace, onChange, onPlace, readAsset, onReplace }: ThingProps) {
  const look = lookOf(thing.parts);
  const [adding, setAdding] = useState(false);
  const change = (patch: Partial<Thing>, summary: string) => onChange({ ...thing, ...patch }, `${summary} ${thing.name}`);
  const setProperties = (properties: Property[], summary: string) => change({ properties: properties.length ? properties : undefined }, summary);
  const props = thing.properties ?? [];
  return (
    <div className="gx-inspector">
      <div className="gx-head">
        <Thumb asset={look} readAsset={readAsset} />
        <div className="grow gx-head-text">
          <Field className="gx-name" value={thing.name} onCommit={(name) => (name.trim() ? change({ name: name.trim() }, "Rename") : "Give it a name.")} />
          <span className="gx-pill">{thing.kind[0].toUpperCase() + thing.kind.slice(1)}</span>
        </div>
      </div>
      <div className="muted small gx-id">
        {thing.id} · {instances === 0 ? "not in this scene" : `${instances} in this scene`}
      </div>
      <div className="row gx-actions">
        <select value={thing.kind} onChange={(e) => change({ kind: e.target.value as ThingKind }, "Change kind of")}>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
        {thing.kind === "character" && (
          <select value={thing.role ?? ""} onChange={(e) => change({ role: e.target.value || undefined }, "Change role of")}>
            {ROLES.map((r) => (
              <option key={r} value={r}>
                {r || "no role"}
              </option>
            ))}
          </select>
        )}
        {canPlace && (
          <button className="small primary" onClick={onPlace} title="Put one in the open scene">
            <Plus size={13} /> Place in scene
          </button>
        )}
      </div>
      {thing.description && <p className="muted small">{thing.description}</p>}

      <section className="gx-section">
        <h4>Properties</h4>
        {props.length === 0 && <p className="muted small">No properties yet.</p>}
        {props.map((p, i) => (
          <div key={p.name} className="gx-row gx-prop" title={p.description}>
            <span className="gx-label">{words(p.name)}</span>
            <ValueField
              value={valueOf(p)}
              onCommit={(v) => setProperties(props.map((q, j) => (j === i ? ({ ...q, ...v } as Property) : q)), `Set ${words(p.name)} on`)}
            />
            {p.unit && <span className="muted small">{p.unit}</span>}
            {(p.type === "number" || p.type === "integer") && rangeOf(p.name) && <span className="muted tiny gx-range">{rangeOf(p.name)}</span>}
            <button className="icon-button gx-reset" title="Remove this property" onClick={() => setProperties(props.filter((_, j) => j !== i), `Remove ${words(p.name)} from`)}>
              <Trash2 size={12} />
            </button>
          </div>
        ))}
        {adding ? (
          <NewProperty
            taken={new Set(props.map((p) => p.name))}
            onAdd={(p) => {
              setProperties([...props, p], `Add ${words(p.name)} to`);
              setAdding(false);
            }}
            onCancel={() => setAdding(false)}
          />
        ) : (
          <button className="small" onClick={() => setAdding(true)}>
            <Plus size={13} /> Add property
          </button>
        )}
      </section>

      <LookSection asset={look} readAsset={readAsset} onReplace={onReplace} />
      <SwitchesSection tags={thing.tags ?? []} onChange={(tags, summary) => change({ tags: tags.length ? tags : undefined }, summary)} />
      <TagsSection tags={thing.tags ?? []} onChange={(tags, summary) => change({ tags: tags.length ? tags : undefined }, summary)} />

      <Fold title="Advanced" open={false}>
        {thing.parts?.length ? (
          <section className="gx-section">
            <h4>Made of</h4>
            <ul className="gx-parts">
              {thing.parts.map((p, i) => (
                <PartRow key={i} part={p} onChange={(next, summary) => change({ parts: thing.parts!.map((q, j) => (j === i ? next : q)) }, summary)} />
              ))}
            </ul>
          </section>
        ) : null}
        <BehaviorList uses={thing.behaviors ?? []} behaviors={behaviors} />
      </Fold>
    </div>
  );
}

function NewProperty({ taken, onAdd, onCancel }: { taken: Set<string>; onAdd: (p: Property) => void; onCancel: () => void }) {
  const [name, setName] = useState("");
  const [type, setType] = useState<"number" | "integer" | "bool" | "text">("number");
  const [unit, setUnit] = useState("");
  const key = name.trim().replace(/\s+(\w)/g, (_, c: string) => c.toUpperCase());
  const problem = !key ? "Name it." : !/^[A-Za-z][A-Za-z0-9_]*$/.test(key) ? "Letters and digits only." : taken.has(key) ? "That name is taken." : null;
  const start: Record<typeof type, Value> = {
    number: { type: "number", value: 0 },
    integer: { type: "integer", value: 0 },
    bool: { type: "bool", value: false },
    text: { type: "text", value: "" },
  };
  return (
    <div className="gx-new-prop">
      <input type="text" autoFocus placeholder="name, e.g. jump height" value={name} onChange={(e) => setName(e.target.value)} />
      <select value={type} onChange={(e) => setType(e.target.value as typeof type)}>
        <option value="number">number</option>
        <option value="integer">whole number</option>
        <option value="bool">yes/no</option>
        <option value="text">text</option>
      </select>
      <input type="text" placeholder="unit (m, s, hp…)" value={unit} onChange={(e) => setUnit(e.target.value)} />
      <button
        className="small primary"
        disabled={problem !== null}
        title={problem ?? undefined}
        onClick={() => onAdd({ name: key, ...start[type], unit: unit.trim() || undefined } as Property)}
      >
        Add
      </button>
      <button className="small" onClick={onCancel}>
        Cancel
      </button>
    </div>
  );
}
