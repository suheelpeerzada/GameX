// The GameX engine's rules (output B: GameX plays the layer itself). Turns a
// scene into actors (things that move or react) and solid boxes (everything
// else with a collider, and solid tiles), moves actors one axis at a time,
// and runs their blocks. No three.js here: Player.tsx draws the world, and
// this file is tested on its own.
import type { Arg, Behavior, BehaviorUse, Block, Entity, Part, Quat, Scene, Thing, Transform, Value, Vec3 } from "./gx";
import { compose, IDENTITY, transformOf, valueOf } from "./gxscene.ts";

export interface Box {
  min: Vec3;
  max: Vec3;
}

/** How the player is steered: side view with gravity, from above, or from behind in 3D. */
export type Control = "platformer" | "topdown" | "third";

/** What an actor is in the game, which decides what it does without blocks. */
export type ActorRole = "player" | "enemy" | "item" | "hazard" | "goal" | "body" | "other";

export interface BehaviorRun {
  use: BehaviorUse;
  def: Behavior;
  state?: string;
  /** Seconds counted per rule, for timers. */
  timers: number[];
}

export interface Actor {
  id: string;
  name: string;
  thing?: Thing;
  /** The entity at its place in the world, with no parent: what Player.tsx draws. */
  entity: Entity;
  role: ActorRole;
  pos: Vec3;
  start: Vec3;
  vel: Vec3;
  /** The middle of its box relative to `pos`, and half the box's size. */
  offset: Vec3;
  half: Vec3;
  /** Falls (in a game with gravity). */
  gravity: boolean;
  /** Others stand on it and bump into it (crates). */
  solid: boolean;
  props: Record<string, Value>;
  runs: BehaviorRun[];
  alive: boolean;
  onGround: boolean;
  /** IDs of the actors it touches, as of the last step. */
  touching: Set<string>;
  damaged: boolean;
  /** Side view: 1 right, -1 left. 3D: turn about the up axis, in radians. */
  facing: number;
  /** Where a patrol around the start point has got to. */
  phase: number;
  /** The way blocks asked it to move this step (along the ground). */
  want: Vec3 | null;
  /** Steps run. */
  age: number;
  /** Switched off in the editor (Behavior): moving, attacking. */
  still: boolean;
  harmless: boolean;
}

export type GameEvent =
  | { type: "sound"; asset: string; volume: number; looped: boolean }
  | { type: "scene"; scene: string }
  | { type: "spawned"; actor: Actor }
  | { type: "removed"; id: string };

/** Where the 2D camera looks: following the player, or fixed. */
export interface View2D {
  follow: boolean;
  center: Vec3;
  /** Half the view's height in metres, when known. */
  halfHeight?: number;
  /** For a fixed view with no camera: what has to fit. */
  size?: [number, number];
}

export interface Input {
  /** Along the ground, in the world (length up to 1). Side view uses x; from above x and y; 3D x and z. */
  move: Vec3;
  jump: boolean;
  /** Jump was pressed this step. */
  jumpPressed: boolean;
  /** Keys pressed this step, lower case ("a", " ", "arrowleft"...). */
  pressed: string[];
}

export interface World {
  flat: boolean;
  control: Control;
  /** Pulls down (−y) this hard, in m/s². 0 from above. */
  gravity: number;
  actors: Actor[];
  player?: Actor;
  statics: Box[];
  grid: Grid;
  /** Lower than this is off the level. */
  floor: number;
  score: number;
  health: number;
  maxHealth: number;
  /** Seconds the player can't be hurt again. */
  hurt: number;
  items: number;
  collected: number;
  over: null | "lost" | "won";
  message: string;
  time: number;
  speed: number;
  jumpSpeed: number;
  view: View2D;
  events: GameEvent[];
  /** Keys pressed this step. */
  pressed: Set<string>;
  things: Record<string, Thing>;
  behaviors: Record<string, Behavior>;
  pixelsPerUnit: number;
  spawned: number;
}

// ---------------------------------------------------------------------------
// Boxes

const EPS = 1e-4;
/** In 2D, boxes reach through all depths. */
const DEEP = 1e6;

function axesOf(flat: boolean): number[] {
  return flat ? [0, 1] : [0, 1, 2];
}

export function overlaps(a: Box, b: Box, flat: boolean, margin = 0): boolean {
  return axesOf(flat).every((i) => a.min[i] < b.max[i] + margin - EPS && a.max[i] > b.min[i] - margin + EPS);
}

export function actorBox(a: Actor): Box {
  const c = [0, 1, 2].map((i) => a.pos[i] + a.offset[i]);
  return { min: c.map((v, i) => v - a.half[i]) as Vec3, max: c.map((v, i) => v + a.half[i]) as Vec3 };
}

/** Boxes bucketed by where they are, so a move only checks what's near. */
export class Grid {
  private cells = new Map<string, Box[]>();
  private big: Box[] = [];
  private flat: boolean;
  private size: number;
  constructor(flat: boolean, size = 4) {
    this.flat = flat;
    this.size = size;
  }

  private range(b: Box): [number, number][] {
    return axesOf(this.flat).map((i) => [Math.floor(b.min[i] / this.size), Math.floor(b.max[i] / this.size)]);
  }

  add(b: Box) {
    const r = this.range(b);
    const count = r.reduce((n, [lo, hi]) => n * (hi - lo + 1), 1);
    if (count > 512) return void this.big.push(b);
    this.each(r, (key) => {
      const list = this.cells.get(key);
      if (list) list.push(b);
      else this.cells.set(key, [b]);
    });
  }

  near(b: Box): Box[] {
    const found = new Set<Box>(this.big);
    this.each(this.range(b), (key) => this.cells.get(key)?.forEach((x) => found.add(x)));
    return [...found];
  }

  private each(r: [number, number][], visit: (key: string) => void) {
    const [x, y, z = [0, 0]] = r;
    for (let i = x[0]; i <= x[1]; i++) for (let j = y[0]; j <= y[1]; j++) for (let k = z[0]; k <= z[1]; k++) visit(`${i},${j},${k}`);
  }
}

/** A rotation's matrix, row by row. */
function matrix([x, y, z, w]: Quat): number[][] {
  return [
    [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
    [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
    [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
  ];
}

/** The world box around a collider part on an entity placed at `t`. */
export function colliderBox(part: Part & { type: "collider" }, t: Transform, flat: boolean): Box {
  let size = [...part.size] as Vec3;
  if (part.shape === "plane") size = flat ? [size[0], size[1], 0.1] : [size[0], 0.1, size[1] || size[2]];
  const local = size.map((s, i) => Math.max(Math.abs(s * t.scale[i]), 0.02) / 2);
  const m = matrix(t.rotation);
  const half = m.map((row) => row.reduce((sum, r, j) => sum + Math.abs(r) * local[j], 0)) as Vec3;
  if (flat) half[2] = DEEP;
  return { min: t.position.map((p, i) => p - half[i]) as Vec3, max: t.position.map((p, i) => p + half[i]) as Vec3 };
}

/** Solid tiles as boxes, a row's neighbours merged into one. */
export function tileBoxes(part: Part & { type: "tiles" }, t: Transform): Box[] {
  const [cw, ch] = part.cell;
  const solid = part.sheets.map((s) => new Set((s.solid ?? []).map(([c, r]) => `${c},${r}`)));
  const cells = part.cells.filter(([, , sheet, col, row]) => solid[sheet]?.has(`${col},${row}`)).map(([x, y]) => [x, y]);
  cells.sort((a, b) => a[1] - b[1] || a[0] - b[0]);
  const out: Box[] = [];
  const sx = Math.abs(t.scale[0]);
  const sy = Math.abs(t.scale[1]);
  for (let i = 0; i < cells.length; ) {
    const [x0, y] = cells[i];
    let n = 1;
    while (i + n < cells.length && cells[i + n][1] === y && cells[i + n][0] === x0 + n) n++;
    out.push({
      min: [t.position[0] + x0 * cw * sx, t.position[1] + y * ch * sy, -DEEP],
      max: [t.position[0] + (x0 + n) * cw * sx, t.position[1] + (y + 1) * ch * sy, DEEP],
    });
    i += n;
  }
  return out;
}

function union(boxes: Box[]): Box | null {
  if (!boxes.length) return null;
  return {
    min: [0, 1, 2].map((i) => Math.min(...boxes.map((b) => b.min[i]))) as Vec3,
    max: [0, 1, 2].map((i) => Math.max(...boxes.map((b) => b.max[i]))) as Vec3,
  };
}

// ---------------------------------------------------------------------------
// Reading the scene

const PLAYER_NAME = /\b(player|hero)\b/i;
const ENEMY_NAME = /\b(enemy|enemies|mob|monster|foe|zombie|slime|bat|ghost)\b/i;
const ITEM_NAME = /\b(coin|gem|star|key|pickup|collectible|fruit|diamond|ring|heart|orb)s?\b/i;
const HAZARD_NAME = /\b(spike|spikes|lava|hazard|trap|death|kill|saw|acid)\b/i;
const GOAL_NAME = /\b(goal|exit|finish|flag|portal|end)\b/i;

/** Names in code style ("PlayerBody", "enemy_2") read as words. */
function wordsOf(name: string): string {
  return name.replace(/([a-z0-9])([A-Z])/g, "$1 $2").replace(/[_\-.]+/g, " ");
}

/** Every part an entity carries, its thing's first. */
function partsOf(e: Entity, things: Record<string, Thing>): Part[] {
  const thing = e.thing ? things[e.thing] : undefined;
  return [...(thing?.parts ?? []), ...(e.parts ?? [])];
}

/** An entity and everything under it (its thing's children too), each with where it is in the world. */
function each(e: Entity, things: Record<string, Thing>, t: Transform, visit: (e: Entity, t: Transform) => void) {
  visit(e, t);
  const thing = e.thing ? things[e.thing] : undefined;
  for (const c of thing?.children ?? []) each(c, things, compose(t, transformOf(c)), visit);
  for (const c of e.children ?? []) each(c, things, compose(t, transformOf(c)), visit);
}

function colliders(e: Entity, things: Record<string, Thing>, t: Transform): (Part & { type: "collider" })[] {
  const out: (Part & { type: "collider" })[] = [];
  each(e, things, t, (x) => partsOf(x, things).forEach((p) => p.type === "collider" && out.push(p)));
  return out;
}

/** An entity moves or reacts in play: it becomes an actor. */
function isActor(e: Entity, things: Record<string, Thing>): boolean {
  const thing = e.thing ? things[e.thing] : undefined;
  if (thing && (thing.kind === "character" || thing.kind === "item" || thing.kind === "tool" || thing.role)) return true;
  if ((thing?.behaviors?.length ?? 0) > 0 || (e.behaviors?.length ?? 0) > 0) return true;
  // Its own colliders only: a level holding a coin isn't itself an actor.
  const own = [...partsOf(e, things), ...(thing?.children ?? []).flatMap((c) => colliders(c, things, IDENTITY))];
  return own.some((c) => c.type === "collider" && (c.body !== "static" || !!c.trigger));
}

function roleOf(e: Entity, thing: Thing | undefined, cols: (Part & { type: "collider" })[]): ActorRole {
  const name = wordsOf(`${e.name} ${thing?.name ?? ""}`);
  if (thing?.role === "player") return "player";
  if (thing?.role === "enemy") return "enemy";
  if (thing?.kind === "item" || ITEM_NAME.test(name)) return "item";
  if (ENEMY_NAME.test(name)) return "enemy";
  if (HAZARD_NAME.test(name)) return "hazard";
  const trigger = cols.length > 0 && cols.every((c) => c.trigger);
  if (trigger && GOAL_NAME.test(name)) return "goal";
  if (cols.some((c) => c.body === "dynamic" && !c.trigger)) return "body";
  return "other";
}

function propsOf(thing: Thing | undefined, e: Entity): Record<string, Value> {
  const out: Record<string, Value> = {};
  for (const p of thing?.properties ?? []) out[p.name] = valueOf(p);
  return { ...out, ...(e.overrides ?? {}) };
}

function num(v: Value | undefined): number | undefined {
  if (!v) return undefined;
  if (v.type === "number" || v.type === "integer") return v.value;
  if (v.type === "bool") return v.value ? 1 : 0;
  if (v.type === "text") {
    const n = Number(v.value);
    return Number.isFinite(n) ? n : undefined;
  }
  return undefined;
}

function text(v: Value | undefined): string | undefined {
  if (!v) return undefined;
  return v.type === "vector" ? v.value.join(",") : String(v.value);
}

/** The first number property whose name matches, best names first. */
function findNumber(props: Record<string, Value>, ...patterns: RegExp[]): { name: string; value: number } | undefined {
  for (const re of patterns) {
    for (const [name, v] of Object.entries(props)) {
      const n = num(v);
      if (n !== undefined && re.test(name)) return { name, value: n };
    }
  }
  return undefined;
}

/** A speed or distance read from a game: in 2D, big values are pixels. */
function metres(w: Pick<World, "flat" | "pixelsPerUnit">, v: number): number {
  return w.flat && Math.abs(v) > 40 ? v / w.pixelsPerUnit : v;
}

/** The editor's Behavior switches, set on the entity or its thing. */
function switches(e: Entity, things: Record<string, Thing>): Set<string> {
  const thing = e.thing ? things[e.thing] : undefined;
  return new Set([...(thing?.tags ?? []), ...(e.tags ?? [])]);
}

function makeActor(e: Entity, t: Transform, w: World): Actor {
  const thing = e.thing ? w.things[e.thing] : undefined;
  const off = switches(e, w.things);
  const cols: Box[] = [];
  const parts: { part: Part; t: Transform }[] = [];
  each(e, w.things, t, (x, xt) => {
    for (const p of partsOf(x, w.things)) {
      parts.push({ part: p, t: xt });
      if (p.type === "collider" && !off.has("no-collide")) cols.push(colliderBox(p, xt, w.flat));
    }
  });
  const colliderParts = parts.map((p) => p.part).filter((p): p is Part & { type: "collider" } => p.type === "collider");
  let box = union(cols);
  if (!box) {
    // No collider: as big as what it looks like, or a person-sized box.
    const looks = parts
      .map(({ part, t: pt }) => {
        if (part.type === "sprite") {
          const [sw, sh] = part.size ?? [1, 1];
          return colliderBox({ type: "collider", shape: "box", size: [sw, sh, 0.1], body: "kinematic" }, pt, w.flat);
        }
        if (part.type === "shape") return colliderBox({ type: "collider", shape: part.shape, size: part.size, body: "kinematic" }, pt, w.flat);
        return null;
      })
      .filter((b): b is Box => b !== null);
    box = union(looks);
  }
  const pos = [...t.position] as Vec3;
  let offset: Vec3;
  let half: Vec3;
  if (box) {
    offset = [0, 1, 2].map((i) => (box!.min[i] + box!.max[i]) / 2 - pos[i]) as Vec3;
    half = [0, 1, 2].map((i) => (box!.max[i] - box!.min[i]) / 2) as Vec3;
  } else if (w.flat) {
    offset = [0, 0, 0];
    half = [0.4, 0.4, DEEP];
  } else {
    // A model with no collider: its origin is usually at its feet.
    offset = [0, 0.9, 0];
    half = [0.4, 0.9, 0.4];
  }
  if (w.flat) {
    offset[2] = 0;
    half[2] = DEEP;
  }
  const role = roleOf(e, thing, colliderParts);
  const moving = colliderParts.some((c) => (c.body === "kinematic" || c.body === "dynamic") && !c.trigger);
  const uses = [...(thing?.behaviors ?? []), ...(e.behaviors ?? [])];
  const runs = uses
    .map((use) => ({ use, def: w.behaviors[use.behavior] }))
    .filter((r): r is { use: BehaviorUse; def: Behavior } => !!r.def)
    .map((r) => ({ ...r, state: r.def.initialState ?? r.def.states?.[0], timers: r.def.rules.map(() => 0) }));
  return {
    id: e.id,
    name: e.name,
    thing,
    entity: { ...e, transform: t },
    role,
    pos,
    start: [...pos] as Vec3,
    vel: [0, 0, 0],
    offset,
    half,
    gravity: role === "player" || ((thing?.kind === "character" || role === "body") && moving) || (role === "enemy" && moving),
    solid: role === "body" && !off.has("no-collide"),
    props: propsOf(thing, e),
    runs,
    alive: e.active !== false,
    onGround: false,
    touching: new Set(),
    damaged: false,
    facing: w.flat ? 1 : 0,
    phase: 0,
    want: null,
    age: 0,
    still: off.has("no-move"),
    harmless: off.has("no-attack"),
  };
}

/** Pulls actors out of the tree: each becomes a top-level entity at its place in the world. */
export function splitScene(scene: Scene, things: Record<string, Thing>): { statics: Entity[]; actors: { entity: Entity; transform: Transform }[] } {
  const actors: { entity: Entity; transform: Transform }[] = [];
  const strip = (list: Entity[], parent: Transform): Entity[] =>
    list.flatMap((e) => {
      const t = compose(parent, transformOf(e));
      if (isActor(e, things)) {
        actors.push({ entity: e, transform: t });
        return [];
      }
      return e.children?.length ? [{ ...e, children: strip(e.children, t) }] : [e];
    });
  return { statics: strip(scene.entities, IDENTITY), actors };
}

/** Sets up a scene for play. */
export function createWorld(
  scene: Scene,
  things: Record<string, Thing>,
  behaviors: Record<string, Behavior>,
  pixelsPerUnit = 100,
): { world: World; statics: Entity[] } {
  const flat = scene.dimensions === "2d";
  const g = Math.hypot(...scene.environment.gravity);
  const w: World = {
    flat,
    control: flat ? "topdown" : "third",
    gravity: 0,
    actors: [],
    statics: [],
    grid: new Grid(flat),
    floor: -50,
    score: 0,
    health: 100,
    maxHealth: 100,
    hurt: 0,
    items: 0,
    collected: 0,
    over: null,
    message: "",
    time: 0,
    speed: 4.5,
    jumpSpeed: 6,
    view: { follow: true, center: [0, 0, 0] },
    events: [],
    pressed: new Set(),
    things,
    behaviors,
    pixelsPerUnit,
    spawned: 0,
  };
  const { statics, actors } = splitScene(scene, things);
  const cameras: { at: Vec3; half?: number; owner?: string }[] = [];
  const sounds = (e: Entity, t: Transform, owner?: string) =>
    each(e, things, t, (x, xt) => {
      for (const p of partsOf(x, things)) {
        if (p.type === "sound" && p.autoplay) w.events.push({ type: "sound", asset: p.asset, volume: p.volume, looped: !!p.looped });
        if (p.type === "camera") cameras.push({ at: [...xt.position] as Vec3, half: p.orthoSize, owner });
      }
    });
  for (const e of statics) {
    const t = transformOf(e);
    each(e, things, t, (x, xt) => {
      for (const p of partsOf(x, things)) {
        if (p.type === "collider" && !p.trigger && !switches(x, things).has("no-collide")) w.statics.push(colliderBox(p, xt, flat));
        if (p.type === "tiles") w.statics.push(...tileBoxes(p, xt));
      }
    });
    sounds(e, t);
  }
  for (const { entity, transform } of actors) {
    const a = makeActor(entity, transform, w);
    w.actors.push(a);
    sounds(entity, transform, a.id);
  }
  for (const b of w.statics) w.grid.add(b);
  for (const a of w.actors) if (a.gravity) settle(w, a);

  w.player =
    w.actors.find((a) => a.role === "player") ??
    w.actors.find((a) => PLAYER_NAME.test(wordsOf(`${a.name} ${a.thing?.name ?? ""}`))) ??
    w.actors.find((a) => a.thing?.kind === "character" && a.role !== "enemy");
  const p = w.player;
  if (p) {
    p.role = "player";
    p.gravity = true;
  }

  // Side view if there's ground under the player; otherwise seen from above.
  if (flat && p) {
    const pb = actorBox(p);
    const under = [...w.statics, ...w.actors.filter((a) => a.solid).map(actorBox)].some(
      (b) => b.min[0] < pb.max[0] && b.max[0] > pb.min[0] && b.max[1] <= pb.min[1] + 0.1 && b.max[1] >= pb.min[1] - 50,
    );
    w.control = under ? "platformer" : "topdown";
  }
  if (w.control !== "topdown") w.gravity = g > 0.01 ? metres(w, g) : flat ? 9.8 : 20;
  else for (const a of w.actors) a.gravity = false;

  // The player's numbers.
  if (p) {
    const height = p.half[1] * 2;
    const speed = findNumber(p.props, /^(move_?|walk_?|run_?|max_?)?speed$/i, /speed/i);
    w.speed = speed ? Math.abs(metres(w, speed.value)) : flat ? Math.min(8, Math.max(0.8, height * 5)) : 4.5;
    const jump = findNumber(p.props, /^jump_?(height|velocity|speed|force|power)?$/i, /jump/i);
    const height3 = Math.min(3, Math.max(0.8, height * 1.6));
    if (jump && /height/i.test(jump.name)) w.jumpSpeed = Math.sqrt(2 * w.gravity * Math.abs(metres(w, jump.value)));
    else if (jump) w.jumpSpeed = Math.abs(metres(w, jump.value));
    else w.jumpSpeed = Math.sqrt(2 * w.gravity * height3);
    const health = findNumber(p.props, /^(health|hp|life|lives)$/i, /health|^hp|lives/i);
    w.maxHealth = w.health = health && health.value > 0 ? health.value : 100;
  }
  w.items = w.actors.filter((a) => a.role === "item").length;

  const all = union([...w.statics.map((b) => (flat ? { min: [b.min[0], b.min[1], 0] as Vec3, max: [b.max[0], b.max[1], 0] as Vec3 } : b)), ...w.actors.map(actorBox)]);
  w.floor = (all?.min[1] ?? 0) - 20;

  // The 2D camera: the scene's own, else everything (a one-screen game), else the player.
  if (flat) {
    const own = cameras.find((c) => c.owner && c.owner === p?.id) ?? cameras[0];
    const size: [number, number] | undefined = all ? [all.max[0] - all.min[0], all.max[1] - all.min[1]] : undefined;
    const middle: Vec3 = all ? [(all.min[0] + all.max[0]) / 2, (all.min[1] + all.max[1]) / 2, 0] : [0, 0, 0];
    const followHalf = p ? Math.min(6, Math.max(1.5, p.half[1] * 2 * 5)) : 5;
    if (own) w.view = { follow: !!own.owner && own.owner === p?.id, center: own.at, halfHeight: own.half ?? followHalf };
    else if (!p || (size && size[0] <= 16 && size[1] <= 16)) w.view = { follow: false, center: middle, size };
    else w.view = { follow: true, center: [...p.pos] as Vec3, halfHeight: followHalf };
  }
  return { world: w, statics };
}

// ---------------------------------------------------------------------------
// Blocks

function argOf(b: Block, ...names: string[]): Arg | undefined {
  for (const n of names) if (b.args?.[n] !== undefined) return b.args[n];
  return undefined;
}

/** An argument's value: as given, or the owner's property it names. */
function resolve(a: Actor, run: BehaviorRun | undefined, arg: Arg | undefined): Value | undefined {
  if (!arg) return undefined;
  if (!("property" in arg)) return arg;
  const name = arg.property;
  if (a.props[name] !== undefined) return a.props[name];
  if (run?.use.params?.[name] !== undefined) return run.use.params[name];
  const p = run?.def.params?.find((x) => x.name === name);
  return p ? valueOf(p) : undefined;
}

function argNumber(a: Actor, run: BehaviorRun | undefined, b: Block, fallback: number, ...names: string[]): number {
  return num(resolve(a, run, argOf(b, ...names))) ?? fallback;
}

/** A speed or distance argument, in metres. */
function argLength(w: World, a: Actor, run: BehaviorRun | undefined, b: Block, fallback: number, ...names: string[]): number {
  const n = num(resolve(a, run, argOf(b, ...names)));
  return n === undefined ? fallback : metres(w, n);
}

function alive(w: World): Actor[] {
  return w.actors.filter((a) => a.alive);
}

function distance(a: Actor, b: Actor, flat: boolean): number {
  return Math.hypot(...axesOf(flat).map((i) => a.pos[i] - b.pos[i]));
}

/** Who a block means: the player by default, a thing by its ID (the nearest one), "self", or a name. */
function targetOf(w: World, a: Actor, run: BehaviorRun | undefined, b: Block): Actor | undefined {
  const v = resolve(a, run, argOf(b, "target", "other", "who", "what"));
  if (!v) return w.player;
  const want = text(v) ?? "";
  if (/^(self|me|this)$/i.test(want)) return a;
  if (want === "player" || (w.player && (w.player.thing?.id === want || w.player.id === want))) return w.player;
  const matches = alive(w).filter((x) => x !== a && (x.thing?.id === want || x.id === want || x.name === want));
  return matches.sort((x, y) => distance(a, x, w.flat) - distance(a, y, w.flat))[0];
}

/** The axes things walk along: x in side view, x/y from above, x/z in 3D. */
function ground(w: World): number[] {
  return w.control === "platformer" ? [0] : w.flat ? [0, 1] : [0, 2];
}

/** Asks `a` to head for `point` at `speed`. */
function seek(w: World, a: Actor, point: Vec3, speed: number, dt: number, away = false) {
  const axes = ground(w);
  const d = axes.map((i) => (point[i] - a.pos[i]) * (away ? -1 : 1));
  const len = Math.hypot(...d);
  const want: Vec3 = [0, 0, 0];
  if (len > 0.05) {
    const s = away ? speed : Math.min(speed, len / Math.max(dt, 1e-3));
    axes.forEach((axis, k) => (want[axis] = (d[k] / len) * s));
  }
  a.want = want;
}

function patrol(w: World, a: Actor, speed: number, radius: number, dt: number) {
  const r = Math.max(radius, 0.1);
  if (w.control === "platformer") {
    if (a.pos[0] > a.start[0] + r) a.facing = -1;
    if (a.pos[0] < a.start[0] - r) a.facing = 1;
    a.want = [a.facing * speed, 0, 0];
    return;
  }
  // A loop around the start point.
  a.phase += (dt * speed) / r;
  const [i, j] = ground(w);
  const point = [...a.start] as Vec3;
  point[i] += Math.cos(a.phase) * r;
  point[j] += Math.sin(a.phase) * r;
  seek(w, a, point, speed * 1.5, dt);
}

function compare(left: number | string | undefined, op: string, right: number | string | undefined): boolean {
  if (left === undefined || right === undefined) return false;
  switch (op.toLowerCase()) {
    case "<":
    case "less":
    case "lessthan":
      return left < right;
    case "<=":
      return left <= right;
    case ">":
    case "greater":
    case "greaterthan":
      return left > right;
    case ">=":
      return left >= right;
    case "!=":
    case "notequal":
      return left !== right;
    default:
      return left === right;
  }
}

function valueOrNumber(v: Value | undefined): number | string | undefined {
  return num(v) ?? text(v);
}

function eventFires(w: World, a: Actor, run: BehaviorRun, b: Block, rule: number, dt: number): boolean {
  switch (b.block) {
    case "start":
      return a.age === 0;
    case "everyFrame":
    case "update":
    case "always":
      return true;
    case "touch":
    case "collide":
    case "hit": {
      if (!argOf(b, "target", "other", "who", "what")) return a.touching.size > 0;
      const t = targetOf(w, a, run, b);
      return !!t && a.touching.has(t.id);
    }
    case "near": {
      const t = targetOf(w, a, run, b);
      return !!t && t.alive && distance(a, t, w.flat) <= argLength(w, a, run, b, 3, "distance", "range", "radius");
    }
    case "timer":
    case "every": {
      const every = Math.max(0.05, argNumber(a, run, b, 1, "seconds", "every", "interval", "time", "delay"));
      run.timers[rule] += dt;
      if (run.timers[rule] < every) return false;
      run.timers[rule] -= every;
      return true;
    }
    case "input":
    case "key": {
      const key = (text(resolve(a, run, argOf(b, "key", "button", "input"))) ?? "").toLowerCase();
      const k = key === "space" ? " " : key;
      return w.pressed.has(k);
    }
    case "damaged":
    case "hurt":
      return a.damaged;
    default:
      return false;
  }
}

function condition(w: World, a: Actor, run: BehaviorRun, b: Block): boolean {
  switch (b.block) {
    case "canSee":
    case "near": {
      const t = targetOf(w, a, run, b);
      return !!t && t.alive && distance(a, t, w.flat) <= argLength(w, a, run, b, 8, "distance", "range", "radius");
    }
    case "farFrom": {
      const t = targetOf(w, a, run, b);
      return !t || !t.alive || distance(a, t, w.flat) > argLength(w, a, run, b, 12, "distance", "range", "radius");
    }
    case "stateIs":
      return run.state === text(resolve(a, run, argOf(b, "state")));
    case "chance": {
      let p = num(resolve(a, run, argOf(b, "chance", "percent", "probability", "value"))) ?? 50;
      if (p > 1) p /= 100;
      return Math.random() < p;
    }
    case "compare": {
      const left = valueOrNumber(resolve(a, run, argOf(b, "a", "left", "property", "value1")));
      const right = valueOrNumber(resolve(a, run, argOf(b, "b", "right", "value", "value2")));
      const op = text(resolve(a, run, argOf(b, "op", "operator", "is"))) ?? "==";
      return compare(left, op, right);
    }
    case "onGround":
      return a.onGround;
    default:
      // A condition GameX doesn't know yet never holds, so nothing unexpected happens.
      return false;
  }
}

function action(w: World, a: Actor, run: BehaviorRun | undefined, b: Block, dt: number) {
  const speedProp = num(a.props.walkSpeed) ?? num(a.props.speed) ?? 1.5;
  switch (b.block) {
    case "patrol":
      return patrol(w, a, argLength(w, a, run, b, metres(w, speedProp), "speed"), argLength(w, a, run, b, 3, "radius", "distance", "range"), dt);
    case "chase":
    case "flee": {
      const t = targetOf(w, a, run, b);
      if (!t || !t.alive) return void (a.want = [0, 0, 0]);
      const speed = argLength(w, a, run, b, metres(w, speedProp), "speed");
      if (w.control === "platformer") a.facing = (t.pos[0] > a.pos[0]) === (b.block === "chase") ? 1 : -1;
      return seek(w, a, t.pos, speed, dt, b.block === "flee");
    }
    case "moveTo": {
      const v = resolve(a, run, argOf(b, "position", "point", "to", "target"));
      let point: Vec3 | undefined;
      if (v?.type === "vector") point = [v.value[0] ?? 0, v.value[1] ?? 0, v.value[2] ?? 0];
      else if (v?.type === "thing") point = targetOf(w, a, run, b)?.pos;
      if (point) seek(w, a, point, argLength(w, a, run, b, metres(w, speedProp), "speed"), dt);
      return;
    }
    case "stop":
      a.want = [0, 0, 0];
      return;
    case "jump":
      if (a.onGround && w.gravity > 0) a.vel[1] = argLength(w, a, run, b, w.jumpSpeed, "speed", "force");
      return;
    case "setState": {
      const s = text(resolve(a, run, argOf(b, "state")));
      if (run && s) run.state = s;
      return;
    }
    case "setProperty": {
      const name = text(resolve(a, run, argOf(b, "property", "name")));
      const v = resolve(a, run, argOf(b, "value", "to"));
      if (!name || !v) return;
      a.props[name] = v;
      if (a === w.player && /^(health|hp|life|lives)$/i.test(name) && num(v) !== undefined) w.health = num(v)!;
      return;
    }
    case "playSound": {
      const asset = text(resolve(a, run, argOf(b, "sound", "asset", "file")));
      if (asset) w.events.push({ type: "sound", asset, volume: argNumber(a, run, b, 1, "volume"), looped: false });
      return;
    }
    case "damage": {
      if (a.harmless) return;
      const t = targetOf(w, a, run, b);
      const fallback = num(a.props.damage) ?? Math.max(1, w.maxHealth / 4);
      if (t) damage(w, t, argNumber(a, run, b, fallback, "amount", "damage", "value"));
      return;
    }
    case "addScore":
      w.score += argNumber(a, run, b, 1, "amount", "points", "value");
      return;
    case "spawn": {
      const id = text(resolve(a, run, argOf(b, "thing", "what", "target")));
      if (id && w.things[id]) spawn(w, id, a.pos);
      return;
    }
    case "destroy": {
      const t = argOf(b, "target", "other", "who") ? targetOf(w, a, run, b) : a;
      if (t) remove(w, t);
      return;
    }
    case "changeScene": {
      const scene = text(resolve(a, run, argOf(b, "scene", "to", "target")));
      if (scene) w.events.push({ type: "scene", scene });
      return;
    }
    default:
      // playAnimation and blocks GameX doesn't know yet: nothing to do here.
      return;
  }
}

function runBlocks(w: World, a: Actor, dt: number) {
  for (const run of a.runs) {
    run.def.rules.forEach((rule, i) => {
      if (!a.alive) return;
      if (rule.state && rule.state !== run.state) return;
      if (!eventFires(w, a, run, rule.when, i, dt)) return;
      if (!(rule.if ?? []).every((c) => condition(w, a, run, c))) return;
      for (const b of rule.then) action(w, a, run, b, dt);
    });
  }
}

// ---------------------------------------------------------------------------
// What happens to actors

export function damage(w: World, t: Actor, amount: number) {
  if (!t.alive || amount <= 0) return;
  t.damaged = true;
  if (t === w.player) {
    if (w.hurt > 0 || w.over) return;
    w.health = Math.max(0, w.health - amount);
    w.hurt = 1;
    if (w.control !== "topdown") t.vel[1] = Math.max(t.vel[1], w.jumpSpeed * 0.4);
    if (w.health <= 0) {
      w.over = "lost";
      w.message = "Game over";
    }
    return;
  }
  const hp = findNumber(t.props, /^(health|hp|life|lives)$/i);
  const left = (hp?.value ?? 0) - amount;
  if (!hp || left <= 0) remove(w, t);
  else t.props[hp.name] = { type: "number", value: left };
}

export function remove(w: World, a: Actor) {
  if (!a.alive) return;
  a.alive = false;
  w.events.push({ type: "removed", id: a.id });
  if (a === w.player && !w.over) {
    w.over = "lost";
    w.message = "Game over";
  }
}

export function spawn(w: World, thingId: string, at: Vec3): Actor {
  const thing = w.things[thingId];
  w.spawned += 1;
  const e: Entity = { id: `${thingId}~${w.spawned}`, name: thing.name, thing: thingId };
  const a = makeActor(e, { ...IDENTITY, position: [...at] as Vec3 }, w);
  if (w.control === "topdown") a.gravity = false;
  w.actors.push(a);
  w.events.push({ type: "spawned", actor: a });
  return a;
}

function collect(w: World, item: Actor) {
  const value = findNumber(item.props, /^(value|points|score|amount|coins?)$/i);
  w.score += value?.value ?? 1;
  w.collected += 1;
  remove(w, item);
  if (w.items > 0 && w.collected >= w.items && !w.actors.some((a) => a.role === "goal") && !w.over) {
    w.over = "won";
    w.message = "You collected everything!";
  }
}

/** What actors do when nobody gave them blocks. */
function defaults(w: World, a: Actor, dt: number) {
  if (a.role === "enemy") {
    const speed = findNumber(a.props, /^(walk_?|move_?)?speed$/i, /speed/i);
    patrol(w, a, speed ? Math.abs(metres(w, speed.value)) : 1.5, 3, dt);
  }
}

/** Touching the player, for actors with no blocks of their own. */
function touchPlayer(w: World, a: Actor, p: Actor, falling: number) {
  if (a.runs.length > 0) return;
  switch (a.role) {
    case "item":
      return collect(w, a);
    case "goal":
      if (!w.over) {
        w.over = "won";
        w.message = "Level complete!";
      }
      return;
    case "hazard":
      if (a.harmless) return;
      return damage(w, p, num(a.props.damage) ?? Math.max(1, w.maxHealth / 4));
    case "enemy": {
      // Landing on an enemy from above defeats it.
      const pb = actorBox(p);
      const eb = actorBox(a);
      if (w.control !== "topdown" && falling < -0.5 && pb.min[1] >= (eb.min[1] + eb.max[1]) / 2) {
        remove(w, a);
        p.vel[1] = w.jumpSpeed * 0.6;
        return;
      }
      if (a.harmless) return;
      return damage(w, p, num(a.props.damage) ?? Math.max(1, w.maxHealth / 4));
    }
  }
}

/** Lifts an actor placed with its feet a little inside the ground on top of it. */
function settle(w: World, a: Actor) {
  for (let tries = 0; tries < 4; tries++) {
    const ab = actorBox(a);
    const inside = w.grid.near(ab).filter((s) => overlaps(ab, s, w.flat));
    const lift = Math.max(0, ...inside.map((s) => s.max[1] - ab.min[1]));
    if (lift <= 0 || lift > a.half[1]) return;
    a.pos[1] += lift;
    a.start[1] = a.pos[1];
  }
}

/** Moves `a` by its velocity one axis at a time, stopping at solids. */
function move(w: World, a: Actor, dt: number) {
  const solidActors = w.actors.filter((x) => x.solid && x.alive && x !== a).map(actorBox);
  a.onGround = false;
  for (const axis of axesOf(w.flat)) {
    const d = a.vel[axis] * dt;
    if (d === 0) continue;
    const before = actorBox(a);
    a.pos[axis] += d;
    const after = actorBox(a);
    const sweep = union([before, after])!;
    let limit: number | null = null;
    for (const s of [...w.grid.near(sweep), ...solidActors]) {
      // Already inside it (placed overlapping): let it walk out.
      if (overlaps(before, s, w.flat) || !overlaps(after, s, w.flat)) continue;
      const edge = d > 0 ? s.min[axis] : s.max[axis];
      if (limit === null || (d > 0 ? edge < limit : edge > limit)) limit = edge;
    }
    if (limit === null) continue;
    a.pos[axis] = d > 0 ? limit - a.offset[axis] - a.half[axis] : limit - a.offset[axis] + a.half[axis];
    a.vel[axis] = 0;
    if (axis === 1 && d < 0) a.onGround = true;
    // Walking into a wall in side view turns it round.
    if (axis === 0 && a !== w.player && w.control === "platformer") a.facing = -a.facing;
  }
}

/** Advances the game by `dt` seconds. */
export function step(w: World, input: Input, dt: number) {
  if (w.over) return;
  w.time += dt;
  w.hurt = Math.max(0, w.hurt - dt);
  w.pressed = new Set(input.pressed.map((k) => k.toLowerCase()));
  const p = w.player;
  const axes = ground(w);
  const actors = alive(w);

  // Blocks and defaults decide how everyone wants to move.
  for (const a of actors) {
    a.want = null;
    if (a.runs.length) runBlocks(w, a, dt);
    else if (a !== p) defaults(w, a, dt);
  }
  // "Damaged" lasts until blocks have seen it once.
  for (const a of actors) a.damaged = false;

  for (const a of actors) {
    if (!a.alive) continue;
    if (a === p) {
      for (const i of axes) a.vel[i] = input.move[i] * w.speed;
      if (w.gravity > 0 && input.jumpPressed && a.onGround) a.vel[1] = w.jumpSpeed;
      // Letting go of jump early makes a shorter jump.
      if (w.gravity > 0 && !input.jump && a.vel[1] > w.jumpSpeed * 0.4) a.vel[1] = w.jumpSpeed * 0.4;
    } else if (a.still) {
      for (const i of axes) a.vel[i] = 0;
    } else if (a.want) {
      for (const i of axes) a.vel[i] = a.want[i];
    } else if (a.role !== "body") {
      for (const i of axes) a.vel[i] = 0;
    } else if (a.onGround) {
      // Crates slide to a stop.
      for (const i of axes) a.vel[i] *= 0.8;
    }
    if (w.flat) {
      if (Math.abs(a.vel[0]) > 0.05) a.facing = Math.sign(a.vel[0]);
    } else {
      const [vx, , vz] = a.vel;
      if (Math.hypot(vx, vz) > 0.1) a.facing = Math.atan2(-vx, -vz);
    }
  }

  // Gravity and movement, in small steps so nothing passes through thin floors.
  const falling = new Map<Actor, number>();
  for (const a of actors) {
    if (!a.alive) continue;
    if (a.gravity && w.gravity > 0) a.vel[1] = Math.max(a.vel[1] - w.gravity * dt, -50);
    falling.set(a, a.vel[1]);
    const fastest = Math.max(...a.vel.map(Math.abs)) * dt;
    const n = Math.min(8, Math.max(1, Math.ceil(fastest / 0.2)));
    let grounded = false;
    for (let s = 0; s < n; s++) {
      move(w, a, dt / n);
      grounded ||= a.onGround;
    }
    a.onGround = grounded;
    if (a.pos[1] < w.floor) {
      if (a === p) {
        // Off the level: back to the start, and it costs some health.
        a.pos = [...a.start] as Vec3;
        a.vel = [0, 0, 0];
        w.hurt = 0;
        damage(w, a, Math.max(1, w.maxHealth / 4));
      } else remove(w, a);
    }
  }

  // Who touches whom.
  const live = alive(w);
  const listens = live.filter((a) => a === p || a.runs.some((r) => r.def.rules.some((x) => ["touch", "collide", "hit"].includes(x.when.block))));
  for (const a of live) a.touching.clear();
  for (const a of listens) {
    const ab = actorBox(a);
    for (const b of live) {
      if (a === b || !overlaps(ab, actorBox(b), w.flat, 0.05)) continue;
      a.touching.add(b.id);
      b.touching.add(a.id);
    }
  }
  if (p?.alive) for (const id of [...p.touching]) {
    const a = w.actors.find((x) => x.id === id);
    if (a?.alive) touchPlayer(w, a, p, falling.get(p) ?? 0);
  }
  for (const a of live) a.age += 1;
  if (p) w.view.center = w.view.follow ? [p.pos[0] + p.offset[0], p.pos[1] + p.offset[1], 0] : w.view.center;
}

/** The behaviors a scene's things and entities use. */
export function behaviorsUsed(scene: Scene, things: Record<string, Thing>): string[] {
  const ids = new Set<string>();
  const visit = (list: Entity[]) =>
    list.forEach((e) => {
      e.behaviors?.forEach((b) => ids.add(b.behavior));
      if (e.thing) things[e.thing]?.behaviors?.forEach((b) => ids.add(b.behavior));
      visit(e.children ?? []);
      if (e.thing) visit(things[e.thing]?.children ?? []);
    });
  visit(scene.entities);
  for (const t of Object.values(things)) t.behaviors?.forEach((b) => ids.add(b.behavior));
  return [...ids];
}
