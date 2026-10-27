// Run with `npm test` (Node's own test runner; Node strips the types).
import assert from "node:assert/strict";
import { test } from "node:test";
import { nearest, solveCalibration, toScreen } from "./picker.ts";

const close = (a: number, b: number) => assert.ok(Math.abs(a - b) < 1e-6, `${a} != ${b}`);

test("two clicks solve both axes", () => {
  // The game is drawn 3x bigger, shifted by (40, 100).
  const c = solveCalibration([
    { wx: 10, wy: 20, sx: 70, sy: 160 },
    { wx: 100, wy: 200, sx: 340, sy: 700 },
  ])!;
  close(c.ax, 3);
  close(c.bx, 40);
  close(c.ay, 3);
  close(c.by, 100);
  const p = toScreen(c, 50, 50);
  close(p.x, 190);
  close(p.y, 250);
});

test("an axis without spread borrows a scale", () => {
  // Both objects on one row: y borrows x's scale.
  const c = solveCalibration([
    { wx: 0, wy: 10, sx: 0, sy: 50 },
    { wx: 10, wy: 10, sx: 20, sy: 50 },
  ])!;
  close(c.ay, 2);
  close(c.by, 30);
  // One click after the camera moved keeps the old scale, refits the offset.
  const moved = solveCalibration([{ wx: 5, wy: 5, sx: 110, sy: 60 }], c)!;
  close(moved.ax, 2);
  close(moved.bx, 100);
  close(moved.by, 50);
});

test("nothing to solve from without spread or history", () => {
  assert.equal(solveCalibration([]), null);
  assert.equal(solveCalibration([{ wx: 1, wy: 1, sx: 5, sy: 5 }]), null);
  assert.equal(solveCalibration([{ wx: 1, wy: 1, sx: 5, sy: 5 }, { wx: 1, wy: 1, sx: 9, sy: 9 }]), null);
});

test("picks the nearest point within reach", () => {
  const points = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 100, y: 100 }];
  assert.equal(nearest(points, { x: 8, y: 1 }, 20), 1);
  assert.equal(nearest(points, { x: 60, y: 60 }, 20), -1);
});
