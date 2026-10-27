// The live changes made this session, and which are kept in the game's code.
import type { LiveField } from "./types";

/** A field changed live, and whether it's been kept in the game's code. */
export interface LiveChange {
  key: string;
  /** The object's class. */
  cls: string;
  field: Pick<LiveField, "name" | "owner" | "isStatic" | "signature">;
  value: string;
  kept: boolean;
}

/** Changes to the same field of the same class count as one: the last value wins. */
function changeKey(cls: string, field: LiveChange["field"]): string {
  return `${field.isStatic ? field.owner : cls}|${field.owner}|${field.name}`;
}

export function recordLiveChange(list: LiveChange[], cls: string, field: LiveChange["field"], value: string): LiveChange[] {
  const key = changeKey(cls, field);
  return [...list.filter((c) => c.key !== key), { key, cls, field, value, kept: false }];
}

/** Marks a change kept, unless the field was changed again meanwhile. */
export function markKept(list: LiveChange[], cls: string, field: LiveChange["field"], value: string): LiveChange[] {
  const key = changeKey(cls, field);
  return list.map((c) => (c.key === key && c.value === value ? { ...c, kept: true } : c));
}
