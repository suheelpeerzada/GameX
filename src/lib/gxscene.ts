// Working with GameX scenes in the window: finding and changing entities
// without mutating, rotations as degrees, a thing's properties with an
// instance's overrides, and blocks read as sentences. No three.js here, so
// it loads with the workspace, not with the 3D view.
import type { Arg, Block, Entity, Property, Quat, Rule, Thing, Transform, Value, Vec3 } from "./gx";

/** At the origin, unrotated, unscaled: an entity with no transform. */
export const IDENTITY: Transform = { position: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] };

// ---------------------------------------------------------------------------
// Entity trees

/** Every entity, depth first, with its parent's ID. */
export function walk(entities: Entity[], parent: string | null = null): { entity: Entity; parent: string | null }[] {
  return entities.flatMap((entity) => [{ entity, parent }, ...walk(entity.children ?? [], entity.id)]);
}

export function findEntity(entities: Entity[], id: string): Entity | undefined {
  return walk(entities).find((w) => w.entity.id === id)?.entity;
}

/** A new tree with `id` replaced by `change(it)`. */
export function updateEntity(entities: Entity[], id: string, change: (e: Entity) => Entity): Entity[] {
  return entities.map((e) => {
    if (e.id === id) return change(e);
    if (!e.children?.length) return e;
    const children = updateEntity(e.children, id, change);
    return children === e.children ? e : { ...e, children };
  });
}

/** A new tree without `id` (and what was under it). */
export function removeEntity(entities: Entity[], id: string): Entity[] {
  return entities
    .filter((e) => e.id !== id)
    .map((e) => (e.children?.length ? { ...e, children: removeEntity(e.children, id) } : e));
}

/** A new tree with `entity` added after `after` (same parent), or at the end. */
export function insertEntity(entities: Entity[], entity: Entity, after?: string): Entity[] {
  if (after) {
    const at = entities.findIndex((e) => e.id === after);
    if (at >= 0) return [...entities.slice(0, at + 1), entity, ...entities.slice(at + 1)];
    let placed = false;
    const next = entities.map((e) => {
      if (placed || !e.children?.length || !findEntity(e.children, after)) return e;
      placed = true;
      return { ...e, children: insertEntity(e.children, entity, after) };
    });
    if (placed) return next;
  }
  return [...entities, entity];
}

/** `base`, or `base-2`, `base-3`... whichever isn't taken. */
export function uniqueId(base: string, taken: Set<string>): string {
  const clean = base.toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 56) || "entity";
  if (!taken.has(clean)) return clean;
  const stem = clean.replace(/-\d+$/, "");
  for (let n = 2; ; n++) if (!taken.has(`${stem}-${n}`)) return `${stem}-${n}`;
}

/** A copy of `entity` and everything under it, with new IDs. */
export function duplicate(entity: Entity, taken: Set<string>): Entity {
  const copy = (e: Entity): Entity => {
    const id = uniqueId(e.id, taken);
    taken.add(id);
    return { ...e, id, children: e.children?.map(copy) };
  };
  const out = copy(entity);
  return { ...out, name: `${entity.name} copy` };
}

export function allIds(entities: Entity[]): Set<string> {
  return new Set(walk(entities).map((w) => w.entity.id));
}

// ---------------------------------------------------------------------------
// Transforms

export function transformOf(e: Entity): Transform {
  return e.transform ?? IDENTITY;
}

const DEG = Math.PI / 180;
const round = (v: number, digits = 4) => {
  const r = Number(v.toFixed(digits));
  return Object.is(r, -0) ? 0 : r;
};

/** Quaternion -> Euler angles in degrees, X then Y then Z (as three.js "XYZ"). */
export function quatToEuler([x, y, z, w]: Quat): Vec3 {
  const m11 = 1 - 2 * (y * y + z * z), m12 = 2 * (x * y - z * w), m13 = 2 * (x * z + y * w);
  const m22 = 1 - 2 * (x * x + z * z), m23 = 2 * (y * z - x * w);
  const m32 = 2 * (y * z + x * w), m33 = 1 - 2 * (x * x + y * y);
  const ey = Math.asin(Math.max(-1, Math.min(1, m13)));
  let ex: number, ez: number;
  if (Math.abs(m13) < 0.9999999) {
    ex = Math.atan2(-m23, m33);
    ez = Math.atan2(-m12, m11);
  } else {
    ex = Math.atan2(m32, m22);
    ez = 0;
  }
  return [round(ex / DEG, 3), round(ey / DEG, 3), round(ez / DEG, 3)];
}

/** Euler angles in degrees (X, Y, Z order) -> quaternion. */
export function eulerToQuat([dx, dy, dz]: Vec3): Quat {
  const [c1, c2, c3] = [Math.cos((dx * DEG) / 2), Math.cos((dy * DEG) / 2), Math.cos((dz * DEG) / 2)];
  const [s1, s2, s3] = [Math.sin((dx * DEG) / 2), Math.sin((dy * DEG) / 2), Math.sin((dz * DEG) / 2)];
  return [
    round(s1 * c2 * c3 + c1 * s2 * s3, 6),
    round(c1 * s2 * c3 - s1 * c2 * s3, 6),
    round(c1 * c2 * s3 + s1 * s2 * c3, 6),
    round(c1 * c2 * c3 - s1 * s2 * s3, 6),
  ];
}

/** A transform with tidy numbers, as saved after a drag. */
export function tidy(t: Transform): Transform {
  return {
    position: t.position.map((v) => round(v)) as Vec3,
    rotation: t.rotation.map((v) => round(v, 6)) as Quat,
    scale: t.scale.map((v) => round(v)) as Vec3,
  };
}

// ---------------------------------------------------------------------------
// Parents: moving an entity under another without it jumping

type Q = Quat;

function qmul(a: Q, b: Q): Q {
  return [
    a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
    a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
    a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
    a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
  ];
}

const qinv = (q: Q): Q => [-q[0], -q[1], -q[2], q[3]];

function rotate(q: Q, v: Vec3): Vec3 {
  const p = qmul(qmul(q, [v[0], v[1], v[2], 0]), qinv(q));
  return [p[0], p[1], p[2]];
}

/** `child` (relative to `parent`) as seen from `parent`'s own space's parent. */
export function compose(parent: Transform, child: Transform): Transform {
  const scaled = child.position.map((v, i) => v * parent.scale[i]) as Vec3;
  const moved = rotate(parent.rotation, scaled);
  return {
    position: moved.map((v, i) => v + parent.position[i]) as Vec3,
    rotation: qmul(parent.rotation, child.rotation),
    scale: child.scale.map((v, i) => v * parent.scale[i]) as Vec3,
  };
}

/** The inverse of `compose`: `world` expressed relative to `parent`. */
function relativeTo(parent: Transform, world: Transform): Transform {
  const inv = qinv(parent.rotation);
  const local = rotate(inv, world.position.map((v, i) => v - parent.position[i]) as Vec3);
  const safe = (s: number) => (Math.abs(s) < 1e-9 ? 1 : s);
  return {
    position: local.map((v, i) => v / safe(parent.scale[i])) as Vec3,
    rotation: qmul(inv, world.rotation),
    scale: world.scale.map((v, i) => v / safe(parent.scale[i])) as Vec3,
  };
}

/** Where an entity is in the scene (its parents' transforms applied). Exact for uniform scales. */
export function worldTransform(entities: Entity[], id: string): Transform | undefined {
  const chain: Entity[] = [];
  const find = (list: Entity[]): boolean =>
    list.some((e) => {
      chain.push(e);
      if (e.id === id || find(e.children ?? [])) return true;
      chain.pop();
      return false;
    });
  if (!find(entities)) return undefined;
  return chain.reduce<Transform>((acc, e) => compose(acc, transformOf(e)), IDENTITY);
}

/**
 * Moves `ids` under `parent` (or to the top level when null), keeping them
 * where they are in the scene. Refuses (returns null) to move something
 * under itself or one of its own children.
 */
export function reparent(entities: Entity[], ids: string[], parent: string | null): Entity[] | null {
  const moving = ids.map((id) => findEntity(entities, id)).filter((e): e is Entity => !!e);
  // Only the top-most of the moved entities move; their children come along.
  const roots = moving.filter((e) => !moving.some((o) => o !== e && findEntity(o.children ?? [], e.id)));
  if (parent && roots.some((e) => e.id === parent || findEntity(e.children ?? [], parent))) return null;
  const parentWorld = parent ? worldTransform(entities, parent) : IDENTITY;
  if (!parentWorld) return null;
  const placed = roots.map((e) => ({
    ...e,
    transform: tidy(relativeTo(parentWorld, worldTransform(entities, e.id)!)),
  }));
  let next = entities;
  for (const e of roots) next = removeEntity(next, e.id);
  if (!parent) return [...next, ...placed];
  return updateEntity(next, parent, (p) => ({ ...p, children: [...(p.children ?? []), ...placed] }));
}

/** Puts `ids` in a new empty entity at their middle, kept where they are. */
export function group(entities: Entity[], ids: string[], name = "Group"): { entities: Entity[]; id: string } | null {
  const worlds = ids.map((id) => worldTransform(entities, id)).filter((t): t is Transform => !!t);
  if (worlds.length === 0) return null;
  const middle = [0, 1, 2].map((i) => worlds.reduce((s, t) => s + t.position[i], 0) / worlds.length) as Vec3;
  const id = uniqueId(name, allIds(entities));
  const holder: Entity = { id, name, transform: tidy({ ...IDENTITY, position: middle }) };
  const withHolder = [...entities, holder];
  const moved = reparent(withHolder, ids, id);
  return moved ? { entities: moved, id } : null;
}

// ---------------------------------------------------------------------------
// Properties

export interface ShownProperty {
  name: string;
  value: Value;
  unit?: string;
  description?: string;
  /** The instance sets its own value. */
  overridden: boolean;
  /** The thing's value, when overridden. */
  base?: Value;
}

/** A thing's properties as an instance sees them. */
export function effectiveProperties(thing: Thing | undefined, entity: Entity): ShownProperty[] {
  const overrides = entity.overrides ?? {};
  const shown: ShownProperty[] = (thing?.properties ?? []).map((p) => {
    const own = overrides[p.name];
    return {
      name: p.name,
      value: own ?? valueOf(p),
      unit: p.unit,
      description: p.description,
      overridden: own !== undefined,
      base: own !== undefined ? valueOf(p) : undefined,
    };
  });
  // Overrides of properties the thing no longer has are still shown, so they can be removed.
  for (const [name, value] of Object.entries(overrides)) {
    if (!shown.some((s) => s.name === name)) shown.push({ name, value, overridden: true });
  }
  return shown;
}

export function valueOf(p: Property): Value {
  return { type: p.type, value: p.value } as Value;
}

export function valueText(v: Value): string {
  switch (v.type) {
    case "bool":
      return v.value ? "yes" : "no";
    case "vector":
      return v.value.join(", ");
    default:
      return String(v.value);
  }
}

/** Reads what was typed as a value of the same type as `like`. */
export function parseValue(like: Value, text: string): Value | { error: string } {
  const t = text.trim();
  switch (like.type) {
    case "number": {
      const n = Number(t);
      return t !== "" && Number.isFinite(n) ? { type: "number", value: n } : { error: "Type a number." };
    }
    case "integer": {
      const n = Number(t);
      return t !== "" && Number.isInteger(n) ? { type: "integer", value: n } : { error: "Type a whole number." };
    }
    case "bool":
      if (/^(yes|true|on|1)$/i.test(t)) return { type: "bool", value: true };
      if (/^(no|false|off|0)$/i.test(t)) return { type: "bool", value: false };
      return { error: "Type yes or no." };
    case "vector": {
      const parts = t.split(/[\s,]+/).filter(Boolean).map(Number);
      return parts.length >= 2 && parts.length <= 4 && parts.every(Number.isFinite)
        ? { type: "vector", value: parts }
        : { error: "Type 2 to 4 numbers, separated by commas." };
    }
    case "color":
      return /^#([0-9a-f]{6}|[0-9a-f]{8})$/i.test(t) ? { type: "color", value: t.toLowerCase() } : { error: "Type a colour like #ff8800." };
    default:
      return { type: like.type, value: t } as Value;
  }
}

// ---------------------------------------------------------------------------
// Blocks as sentences (the read-only view until the blocks editor, milestone 4)

const BLOCK_WORDS: Record<string, string> = {
  start: "on start",
  everyFrame: "every frame",
  touch: "on touching",
  near: "when near",
  input: "on input",
  timer: "on timer",
  damaged: "when damaged",
  canSee: "can see",
  farFrom: "is far from",
  stateIs: "state is",
  chance: "random chance",
  compare: "compare",
  patrol: "patrol",
  chase: "chase",
  flee: "flee from",
  moveTo: "move to",
  setState: "switch to",
  setProperty: "set",
  playAnimation: "play animation",
  playSound: "play sound",
  damage: "damage",
  addScore: "add to score",
  spawn: "spawn",
  destroy: "remove itself",
  changeScene: "go to scene",
};

/** "chaseSpeed" -> "chase speed" */
export function words(name: string): string {
  return name.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/[_-]+/g, " ").toLowerCase();
}

function argText(a: Arg): string {
  return "property" in a ? words(a.property) : valueText(a);
}

export function blockText(b: Block): string {
  const head = BLOCK_WORDS[b.block] ?? words(b.block);
  // Who or what it's about first ("chase player, speed…"); files store args alphabetically.
  const rank = (k: string) => (k === "target" ? 0 : k === "state" ? 1 : 2);
  const args = Object.entries(b.args ?? {}).sort(([a], [b]) => rank(a) - rank(b));
  if (args.length === 0) return head;
  return `${head} ${args.map(([k, a]) => (k === "target" || k === "state" ? argText(a) : `${words(k)} ${argText(a)}`)).join(", ")}`;
}

/** "In chase: every frame, if is far from player, distance give up distance → switch to patrol" */
export function ruleText(r: Rule): string {
  const when = blockText(r.when);
  const cond = r.if?.length ? `, if ${r.if.map(blockText).join(" and ")}` : "";
  const then = r.then.map(blockText).join(", then ") || "nothing";
  return `${r.state ? `In ${r.state}: ` : ""}${when}${cond} → ${then}`;
}
