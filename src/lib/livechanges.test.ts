// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { markKept, recordLiveChange } from "./livechanges.ts";

const hp = { name: "hp", owner: "g.Actor", isStatic: false, signature: "I" };
const level = { name: "level", owner: "g.Game", isStatic: true, signature: "I" };

test("one entry per field and class, last value wins", () => {
  let list = recordLiveChange([], "g.Hero", hp, "50");
  list = recordLiveChange(list, "g.Enemy", hp, "10");
  list = recordLiveChange(list, "g.Hero", hp, "99");
  assert.deepEqual(list.map((c) => [c.cls, c.value]), [["g.Enemy", "10"], ["g.Hero", "99"]]);
  // A static field is one value whichever object it was changed through.
  list = recordLiveChange(list, "g.Hero", level, "2");
  list = recordLiveChange(list, "g.Enemy", level, "3");
  assert.equal(list.filter((c) => c.field.name === "level").length, 1);
});

test("keeping marks only the value that was kept", () => {
  let list = recordLiveChange([], "g.Hero", hp, "50");
  list = markKept(list, "g.Hero", hp, "50");
  assert.equal(list[0].kept, true);
  // Changed again: not kept until kept again.
  list = recordLiveChange(list, "g.Hero", hp, "60");
  assert.equal(list[0].kept, false);
  // A keep of the old value finishing late doesn't mark the new one.
  list = markKept(list, "g.Hero", hp, "50");
  assert.equal(list[0].kept, false);
});
