import type { CapabilityStatus, Category } from "./types";

/** Display order and plain-language meaning of each category. */
export const categories: { id: Category; label: string; hint: string }[] = [
  { id: "characters", label: "Characters", hint: "Players, enemies and anything else that acts" },
  { id: "core", label: "Core", hint: "Game setup, main loop, settings and package files" },
  { id: "map", label: "Map", hint: "Levels, rooms, tiles and backgrounds" },
  { id: "items", label: "Items", hint: "Things to pick up, carry or use" },
  { id: "assets", label: "Assets", hint: "Other images, video and models" },
  { id: "audio", label: "Audio", hint: "Music and sound effects" },
  { id: "ui", label: "UI", hint: "Menus, buttons, icons, fonts and text" },
  { id: "code", label: "Code", hint: "Classes, scripts and libraries" },
  { id: "unsorted", label: "Unsorted", hint: "Anything GameX couldn't place" },
];

export const categoryLabel = Object.fromEntries(categories.map((c) => [c.id, c.label])) as Record<
  Category,
  string
>;

export const statusLabel: Record<CapabilityStatus, string> = {
  supported: "Supported",
  partial: "Partial",
  unsupported: "Not supported",
  unknown: "Unknown",
};
