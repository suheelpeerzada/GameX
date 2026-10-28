// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { projectFeatures } from "./features.ts";
import type { GameProject } from "./types.ts";

const analyzed = (engine: string) => ({ engine: { id: engine } }) as GameProject;

test("live editing only for Java/Kotlin APKs", () => {
  assert.equal(projectFeatures("apk", analyzed("android-native")).live, true);
  assert.equal(projectFeatures("apk", analyzed("unity")).live, false);
  assert.equal(projectFeatures("apk", analyzed("godot")).live, false);
  assert.equal(projectFeatures("apk", null).live, false);
  assert.equal(projectFeatures("folder", analyzed("android-native")).live, false);
});

test("run and build only for APKs", () => {
  assert.deepEqual(projectFeatures("apk", analyzed("unity")), { run: true, build: true, live: false });
  assert.deepEqual(projectFeatures("zip", analyzed("godot")), { run: false, build: false, live: false });
});
