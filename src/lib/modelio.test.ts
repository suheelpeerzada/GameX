// Run with `npm test`. Formats that need a browser (textures, glTF) are
// checked in the app itself.
import assert from "node:assert/strict";
import { test } from "node:test";
import { bounds, exportModel, fitInto, isTextVariant, loadModel, modelStats } from "./modelio.ts";

const CUBE_OBJ = `# a unit cube
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
v 1 0 1
v 1 1 1
v 0 1 1
f 1 2 3 4
f 5 8 7 6
f 1 5 6 2
f 2 6 7 3
f 3 7 8 4
f 5 1 4 8
`;
const bytes = (s: string) => new TextEncoder().encode(s).buffer as ArrayBuffer;

test("reads an OBJ and counts what's in it", async () => {
  const model = await loadModel("obj", bytes(CUBE_OBJ), new Map());
  const stats = modelStats(model.root);
  assert.equal(stats.meshes, 1);
  assert.equal(stats.triangles, 12);
  assert.deepEqual(stats.size, [1, 1, 1]);
  assert.deepEqual(model.missing, []);
});

test("reports a missing material file", async () => {
  const model = await loadModel("obj", bytes(`mtllib hero.mtl\n${CUBE_OBJ}`), new Map());
  assert.deepEqual(model.missing, ["hero.mtl"]);
});

test("converts between formats, keeping the shape", async () => {
  const model = await loadModel("obj", bytes(CUBE_OBJ), new Map());
  for (const [format, text] of [["stl", false], ["stl", true], ["ply", false], ["ply", true], ["obj", true]] as const) {
    const out = await exportModel(model.root, [], format, text);
    if (format !== "obj") assert.equal(isTextVariant(format, out), text, `${format} text=${text}`);
    const back = await loadModel(format, out.slice().buffer as ArrayBuffer, new Map());
    assert.equal(modelStats(back.root).triangles, 12, format);
    assert.deepEqual(bounds(back.root), { min: [0, 0, 0], max: [1, 1, 1] }, format);
  }
});

test("fitting moves and scales the written model", async () => {
  const model = await loadModel("obj", bytes(CUBE_OBJ), new Map());
  const fitted = fitInto(model.root, { min: [-2, 5, -2], max: [2, 9, 2] });
  const out = await exportModel(fitted, [], "obj");
  const back = await loadModel("obj", out.slice().buffer as ArrayBuffer, new Map());
  assert.deepEqual(bounds(back.root), { min: [-2, 5, -2], max: [2, 9, 2] });
});
