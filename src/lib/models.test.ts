// Run with `npm test` (Node's own test runner; Node strips the types).
import assert from "node:assert/strict";
import { test } from "node:test";
import { fitTransform, isModelPath, matchAuxFile, replacePlan } from "./models.ts";

test("knows model files by extension", () => {
  assert.ok(isModelPath("apktool/assets/hero.GLB"));
  assert.ok(isModelPath("files/a/b.3ds"));
  assert.ok(!isModelPath("files/hero.png"));
  assert.ok(!isModelPath("files/obj"));
});

test("plans how a replacement becomes the game's format", () => {
  assert.deepEqual(replacePlan("fbx", "fbx", false), { kind: "copy" });
  assert.deepEqual(replacePlan("glb", "obj", false), { kind: "convert" });
  assert.deepEqual(replacePlan("obj", "obj", true), { kind: "convert" });
  assert.equal(replacePlan("obj", "fbx", false).kind, "unsupported");
  assert.equal(replacePlan("fbx", "fbx", true).kind, "unsupported");
});

test("fits a model where the original stood", () => {
  // A 10-unit model centered on 0, replaced by one 2 units big, off to the side.
  const original = { min: [-5, 0, -5], max: [5, 10, 5] } as const;
  const replacement = { min: [100, 1, 100], max: [102, 3, 101] } as const;
  const { scale, offset } = fitTransform(
    { min: [...replacement.min], max: [...replacement.max] },
    { min: [...original.min], max: [...original.max] },
  );
  assert.equal(scale, 5);
  const moved = (v: number, i: number) => v * scale + offset[i];
  // Centered on x/z, standing on the same floor.
  assert.equal((moved(100, 0) + moved(102, 0)) / 2, 0);
  assert.equal((moved(100, 2) + moved(101, 2)) / 2, 0);
  assert.equal(moved(1, 1), 0);
  // An empty model isn't scaled.
  assert.equal(fitTransform({ min: [0, 0, 0], max: [0, 0, 0] }, { min: [0, 0, 0], max: [1, 1, 1] }).scale, 1);
});

test("finds the side files a model asks for", () => {
  const files = new Map([
    ["hero.mtl", "blob:1"],
    ["textures/skin.png", "blob:2"],
  ]);
  assert.equal(matchAuxFile("hero.mtl", files), "blob:1");
  assert.equal(matchAuxFile("./textures/skin.png", files), "blob:2");
  assert.equal(matchAuxFile("C:\\Users\\artist\\Desktop\\SKIN.png", files), "blob:2");
  assert.equal(matchAuxFile("textures%2Fskin.png", files), "blob:2");
  assert.equal(matchAuxFile("missing.png", files), undefined);
});
