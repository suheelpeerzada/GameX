// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import type { Behavior, Entity, Part, Scene, Thing } from "./gx.ts";
import { actorBox, createWorld, splitScene, step, tileBoxes, type Input } from "./gxengine.ts";

const idle: Input = { move: [0, 0, 0], jump: false, jumpPressed: false, pressed: [] };
const right: Input = { ...idle, move: [1, 0, 0] };

function scene(dimensions: "2d" | "3d", entities: Entity[]): Scene {
  return { format: 1, id: "s", name: "S", dimensions, environment: { background: "#000000", ambient: "#ffffff", gravity: [0, -9.8, 0] }, entities };
}

const at = (x: number, y: number, z = 0) => ({ position: [x, y, z] as [number, number, number], rotation: [0, 0, 0, 1] as [number, number, number, number], scale: [1, 1, 1] as [number, number, number] });
const box = (w: number, h: number, body: "static" | "kinematic" | "dynamic" = "static", trigger = false): Part => ({ type: "collider", shape: "box", size: [w, h, 1], body, trigger });

const things: Record<string, Thing> = {
  player: { format: 1, id: "player", name: "Player", kind: "character", role: "player", properties: [{ name: "speed", type: "number", value: 300 }], parts: [box(0.5, 1, "kinematic")] },
  coin: { format: 1, id: "coin", name: "Coin", kind: "item", properties: [{ name: "value", type: "integer", value: 5 }], parts: [box(0.3, 0.3, "static", true)] },
  slime: { format: 1, id: "slime", name: "Slime", kind: "character", role: "enemy", parts: [box(0.5, 0.5, "kinematic")] },
};

const floor: Entity = { id: "floor", name: "Floor", transform: at(0, -0.5), parts: [box(40, 1)] };

function run(w: ReturnType<typeof createWorld>["world"], input: Input, seconds: number) {
  for (let t = 0; t < seconds; t += 1 / 60) step(w, input, 1 / 60);
}

test("a 2D level with ground under the player is a platformer; the player lands and walks", () => {
  const { world } = createWorld(scene("2d", [floor, { id: "p", name: "Player", thing: "player", transform: at(0, 2) }]), things, {});
  assert.equal(world.control, "platformer");
  // 300 is pixels per second in a 2D game: 3 m/s.
  assert.equal(world.speed, 3);
  run(world, idle, 1.5);
  const p = world.player!;
  assert.ok(p.onGround);
  assert.ok(Math.abs(actorBox(p).min[1]) < 1e-3, `feet at ${actorBox(p).min[1]}`);
  run(world, right, 1);
  assert.ok(p.pos[0] > 2.8 && p.pos[0] < 3.1, `x ${p.pos[0]}`);
});

test("walls stop the player", () => {
  const wall: Entity = { id: "wall", name: "Wall", transform: at(2, 1), parts: [box(1, 2)] };
  const { world } = createWorld(scene("2d", [floor, wall, { id: "p", name: "Player", thing: "player", transform: at(0, 0.5) }]), things, {});
  run(world, right, 2);
  assert.ok(Math.abs(actorBox(world.player!).max[0] - 1.5) < 1e-3);
});

test("a 2D scene with nothing under the player is seen from above, with no gravity", () => {
  const { world } = createWorld(scene("2d", [{ id: "p", name: "Player", thing: "player", transform: at(0, 0) }]), things, {});
  assert.equal(world.control, "topdown");
  run(world, { ...idle, move: [0, 1, 0] }, 1);
  assert.ok(world.player!.pos[1] > 2.9);
});

test("touching an item collects it; collecting everything wins", () => {
  const { world } = createWorld(
    scene("2d", [floor, { id: "p", name: "Player", thing: "player", transform: at(0, 0.5) }, { id: "c", name: "Coin", thing: "coin", transform: at(1, 0.5) }]),
    things,
    {},
  );
  assert.equal(world.items, 1);
  run(world, right, 1);
  assert.equal(world.score, 5);
  assert.equal(world.over, "won");
  assert.ok(world.events.some((e) => e.type === "removed" && e.id === "c"));
});

test("enemies hurt on touch, once a second; landing on one defeats it", () => {
  const { world } = createWorld(
    scene("2d", [floor, { id: "p", name: "Player", thing: "player", transform: at(0, 0.5) }, { id: "e", name: "Slime", thing: "slime", transform: at(0.4, 0.25) }]),
    things,
    {},
  );
  run(world, idle, 0.1);
  assert.equal(world.health, 75);
  run(world, idle, 0.3);
  assert.equal(world.health, 75);

  const { world: w2 } = createWorld(
    scene("2d", [floor, { id: "p", name: "Player", thing: "player", transform: at(0, 1.3) }, { id: "e", name: "Slime", thing: "slime", transform: at(0, 0.25) }]),
    things,
    {},
  );
  run(w2, idle, 1);
  assert.equal(w2.actors.find((a) => a.id === "e")!.alive, false);
  assert.equal(w2.health, 100);
});

test("blocks: a guard patrols, chases the player it can see, and gives up", () => {
  const chase: Behavior = {
    format: 1,
    id: "chase",
    name: "Chase",
    states: ["patrol", "chase"],
    initialState: "patrol",
    rules: [
      { state: "patrol", when: { block: "everyFrame" }, then: [{ block: "patrol", args: { speed: { type: "number", value: 1 }, radius: { type: "number", value: 2 } } }] },
      { state: "patrol", when: { block: "everyFrame" }, if: [{ block: "canSee", args: { target: { type: "thing", value: "player" }, distance: { type: "number", value: 5 } } }], then: [{ block: "setState", args: { state: { type: "text", value: "chase" } } }] },
      { state: "chase", when: { block: "everyFrame" }, then: [{ block: "chase", args: { target: { type: "thing", value: "player" }, speed: { type: "number", value: 2 } } }] },
    ],
  };
  const guard: Thing = { format: 1, id: "guard", name: "Guard", kind: "character", role: "enemy", parts: [box(0.8, 1.8, "kinematic")], behaviors: [{ behavior: "chase" }] };
  const all = { ...things, guard };
  const ground: Entity = { id: "ground", name: "Ground", transform: at(0, -0.5, 0), parts: [{ type: "collider", shape: "box", size: [60, 1, 60], body: "static" }] };
  const { world } = createWorld(
    scene("3d", [ground, { id: "p", name: "Player", thing: "player", transform: at(0, 0.5, 0) }, { id: "g", name: "Guard", thing: "guard", transform: at(20, 0.9, 0) }]),
    all,
    { chase },
  );
  assert.equal(world.control, "third");
  const g = world.actors.find((a) => a.id === "g")!;
  run(world, idle, 1);
  assert.equal(g.runs[0].state, "patrol");
  assert.ok(Math.hypot(g.pos[0] - 20, g.pos[2]) <= 2.5);
  g.pos = [4, g.pos[1], 0];
  run(world, idle, 0.5);
  assert.equal(g.runs[0].state, "chase");
  assert.ok(g.pos[0] < 4);
});

test("solid tiles become boxes, neighbours in a row merged", () => {
  const tiles: Part & { type: "tiles" } = {
    type: "tiles",
    cell: [0.16, 0.16],
    sheets: [{ asset: "t.png", tile: [16, 16], solid: [[0, 0]] }],
    cells: [
      [0, 0, 0, 0, 0, 0],
      [1, 0, 0, 0, 0, 0],
      [2, 0, 0, 1, 0, 0],
      [3, 0, 0, 0, 0, 0],
    ],
  };
  const boxes = tileBoxes(tiles, at(1, 0));
  assert.equal(boxes.length, 2);
  assert.deepEqual([boxes[0].min[0], boxes[0].max[0]], [1, 1.32]);
  assert.ok(Math.abs(boxes[1].min[0] - 1.48) < 1e-9);
});

test("actors inside other entities come out at their place in the world", () => {
  const level: Entity = { id: "level", name: "Level", transform: at(10, 0), children: [{ id: "c", name: "Coin", thing: "coin", transform: at(1, 2) }, floor] };
  const { statics, actors } = splitScene(scene("2d", [level]), things);
  assert.equal(actors.length, 1);
  assert.deepEqual(actors[0].transform.position, [11, 2, 0]);
  assert.deepEqual(statics[0].children?.map((c) => c.id), ["floor"]);
});

test("the editor's Behavior switches: an enemy that can't attack or move", () => {
  const tame = { ...things.slime, id: "tame", tags: ["no-attack", "no-move"] };
  const { world } = createWorld(
    scene("2d", [floor, { id: "p", name: "Player", thing: "player", transform: at(0, 0.5) }, { id: "e", name: "Tame", thing: "tame", transform: at(0.4, 0.25) }]),
    { ...things, tame },
    {},
  );
  const e = world.actors.find((a) => a.id === "e")!;
  const x = e.pos[0];
  run(world, idle, 1);
  assert.equal(world.health, 100);
  assert.equal(e.pos[0], x);
});
