// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import type { Entity, Thing } from "./gx.ts";
import {
  allIds,
  duplicate,
  effectiveProperties,
  eulerToQuat,
  findEntity,
  group,
  insertEntity,
  reparent,
  transformOf,
  worldTransform,
  parseValue,
  quatToEuler,
  removeEntity,
  ruleText,
  uniqueId,
  updateEntity,
  walk,
} from "./gxscene.ts";

const tree: Entity[] = [
  { id: "ground", name: "Ground" },
  { id: "walls", name: "Walls", children: [{ id: "wall-n", name: "North" }, { id: "wall-s", name: "South" }] },
];

test("finds, changes, adds and removes entities without touching the original", () => {
  assert.equal(walk(tree).length, 4);
  assert.equal(walk(tree).find((w) => w.entity.id === "wall-s")?.parent, "walls");
  const renamed = updateEntity(tree, "wall-s", (e) => ({ ...e, name: "Back" }));
  assert.equal(findEntity(renamed, "wall-s")?.name, "Back");
  assert.equal(findEntity(tree, "wall-s")?.name, "South");
  assert.equal(renamed[0], tree[0], "untouched branches are reused");

  const added = insertEntity(tree, { id: "wall-e", name: "East" }, "wall-n");
  assert.deepEqual(added[1].children?.map((c) => c.id), ["wall-n", "wall-e", "wall-s"]);
  assert.equal(insertEntity(tree, { id: "x", name: "X" }).at(-1)?.id, "x");

  const removed = removeEntity(tree, "walls");
  assert.deepEqual([...allIds(removed)], ["ground"]);
});

test("new IDs never clash", () => {
  const taken = allIds(tree);
  assert.equal(uniqueId("Wall N", taken), "wall-n-2");
  assert.equal(uniqueId("crate", taken), "crate");
  const copy = duplicate(tree[1], taken);
  assert.equal(copy.name, "Walls copy");
  assert.deepEqual([copy.id, ...(copy.children ?? []).map((c) => c.id)], ["walls-2", "wall-n-2", "wall-s-2"]);
});

test("rotations round-trip through degrees", () => {
  for (const e of [[0, 0, 0], [0, 30, 0], [-30, 0, 0], [10, 20, 30], [0, 0, 90]] as [number, number, number][]) {
    const back = quatToEuler(eulerToQuat(e));
    back.forEach((v, i) => assert.ok(Math.abs(v - e[i]) < 0.01, `${e} -> ${back}`));
  }
  assert.deepEqual(eulerToQuat([0, 0, 0]), [0, 0, 0, 1]);
});

test("instances show the thing's properties with their own overrides", () => {
  const guard: Thing = {
    format: 1,
    id: "guard",
    name: "Guard",
    kind: "character",
    properties: [
      { name: "health", type: "number", value: 50, unit: "hp" },
      { name: "walkSpeed", type: "number", value: 1.5, unit: "m/s" },
    ],
  };
  const g2: Entity = { id: "g2", name: "Guard 2", thing: "guard", overrides: { walkSpeed: { type: "number", value: 2 }, old: { type: "bool", value: true } } };
  const shown = effectiveProperties(guard, g2);
  assert.deepEqual(shown.map((s) => [s.name, s.value.value, s.overridden]), [
    ["health", 50, false],
    ["walkSpeed", 2, true],
    ["old", true, true],
  ]);
  assert.equal(shown[1].base?.value, 1.5);
});

test("moving under another parent keeps things where they are", () => {
  const scene: Entity[] = [
    { id: "box", name: "Box", transform: { position: [5, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] } },
    // Turned 90° about Y and twice as big.
    { id: "table", name: "Table", transform: { position: [2, 1, 0], rotation: eulerToQuat([0, 90, 0]), scale: [2, 2, 2] } },
  ];
  const before = worldTransform(scene, "box")!;
  const moved = reparent(scene, ["box"], "table")!;
  assert.equal(findEntity(moved, "table")?.children?.[0].id, "box");
  const after = worldTransform(moved, "box")!;
  after.position.forEach((v, i) => assert.ok(Math.abs(v - before.position[i]) < 1e-3, `${after.position} vs ${before.position}`));
  assert.ok(Math.abs(transformOf(findEntity(moved, "box")!).scale[0] - 0.5) < 1e-6);
  // And back to the top.
  const back = reparent(moved, ["box"], null)!;
  assert.deepEqual(transformOf(findEntity(back, "box")!).position, [5, 0, 0]);
  // Never under itself or its own child.
  assert.equal(reparent(moved, ["table"], "box"), null);
});

test("grouping puts things in a new parent at their middle", () => {
  const scene: Entity[] = [
    { id: "a", name: "A", transform: { position: [0, 0, 0], rotation: [0, 0, 0, 1], scale: [1, 1, 1] } },
    { id: "b", name: "B", transform: { position: [4, 0, 2], rotation: [0, 0, 0, 1], scale: [1, 1, 1] } },
  ];
  const g = group(scene, ["a", "b"])!;
  assert.equal(g.entities.length, 1);
  assert.deepEqual(transformOf(g.entities[0]).position, [2, 0, 1]);
  assert.deepEqual(worldTransform(g.entities, "b")?.position, [4, 0, 2]);
});

test("typed values are checked against their type", () => {
  assert.deepEqual(parseValue({ type: "number", value: 1 }, " 3.5 "), { type: "number", value: 3.5 });
  assert.ok("error" in parseValue({ type: "number", value: 1 }, "fast"));
  assert.ok("error" in parseValue({ type: "integer", value: 1 }, "2.5"));
  assert.deepEqual(parseValue({ type: "bool", value: true }, "no"), { type: "bool", value: false });
  assert.deepEqual(parseValue({ type: "vector", value: [] }, "1, 2,3"), { type: "vector", value: [1, 2, 3] });
  assert.ok("error" in parseValue({ type: "color", value: "#000000" }, "red"));
});

test("rules read as sentences", () => {
  const text = ruleText({
    state: "chase",
    when: { block: "everyFrame" },
    // Stored alphabetically, as the files have it; read with the target first.
    if: [{ block: "farFrom", args: { distance: { property: "giveUpDistance" }, target: { type: "thing", value: "player" } } }],
    then: [{ block: "setState", args: { state: { type: "text", value: "patrol" } } }],
  });
  assert.equal(text, "In chase: every frame, if is far from player, distance give up distance → switch to patrol");
});
