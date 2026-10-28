# The GameX format, version 1

Every game GameX translates, and every project made by hand, is stored this
way. The workspace edits it and the GameX engine plays it; engine readers
write it. The code is `src-tauri/src/gx/` (Rust) and `src/lib/gx.ts` (the
window). The plan behind it is `docs/PLAN.md`.

## The project folder

Projects live in `Documents\GameX\Projects\<name>\`:

```
project.json                     the project: name, games in it, settings
world\<id>.scene.json            one scene each
things\characters\<id>.thing.json
things\items\  things\tools\  things\props\  things\other\
behavior\<id>.blocks.json        behavior made of blocks
behavior\<id>.gxs                GameX scripts (milestone 4)
assets\models\ textures\ sprites\ animations\ audio\ fonts\
sources\<game>\                  one folder per translated game
  original\                      untouched copy
  decompiled\                    the reader's working files
  origin.json                    where each element came from
history\                         undo (log.json + earlier versions)
```

All files are UTF-8 JSON, indented with two spaces, so they read and diff
well. Every file has a `"format"` number; GameX refuses files with a higher
number than it knows instead of misreading them.

## Rules that hold everywhere

- **IDs** use `a-z`, `0-9`, `-` and `_`, at most 64 characters. A scene,
  thing or behavior's ID is also its file name. Entity IDs are unique within
  their scene.
- **Space:** right-handed, Y up, metres. Rotations are quaternions
  `[x, y, z, w]`. 2D scenes use the same axes, seen from the front, with
  `pixelsPerUnit` turning sprite pixels into metres.
- **Colours:** `"#rrggbb"` or `"#rrggbbaa"`.
- **Asset paths** are relative to `assets\`: `"models/door.glb"`.
- **Defaults are left out:** an entity with no `transform` sits at the
  origin, unrotated, at scale 1; one with no `active` is active.

## project.json

```json
{
  "format": 1,
  "id": "haunted-house-a1b2c3",
  "name": "Haunted House",
  "sources": [
    { "id": "granny", "name": "Granny", "engine": "unity", "engineVersion": "2021.3.11f1" }
  ],
  "settings": { "startScene": "house", "pixelsPerUnit": 100.0 }
}
```

## Values and properties

A value always says its type, so nothing is guessed:

| Type | Example |
| --- | --- |
| `number` | `{"type": "number", "value": 7.1}` |
| `integer` | `{"type": "integer", "value": 3}` |
| `bool` | `{"type": "bool", "value": true}` |
| `text` | `{"type": "text", "value": "chase"}` |
| `vector` | `{"type": "vector", "value": [0, 1, 0]}` |
| `color` | `{"type": "color", "value": "#ff8800"}` |
| `thing` | `{"type": "thing", "value": "player"}` |
| `asset` | `{"type": "asset", "value": "audio/creak.ogg"}` |

A property is a named value with an optional unit:
`{"name": "chaseSpeed", "type": "number", "value": 3.5, "unit": "m/s"}`.

## Scenes

```json
{
  "format": 1,
  "id": "courtyard",
  "name": "Courtyard",
  "dimensions": "3d",
  "environment": { "background": "#87a8c8", "ambient": "#404040", "gravity": [0, -9.81, 0] },
  "entities": [
    {
      "id": "guard-2",
      "name": "Guard 2",
      "thing": "guard",
      "transform": { "position": [6, 0.9, -6], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1] },
      "overrides": { "walkSpeed": { "type": "number", "value": 2.0 } }
    }
  ]
}
```

An entity has an `id`, a `name`, and any of: `thing` (it's an instance of
that thing), `transform`, `active`, `tags`, `parts`, `behaviors`,
`overrides` (property values for this instance only) and `children`
(placed relative to it).

## Parts

What an entity or thing is made of; each has a `"type"`:

| Type | Fields |
| --- | --- |
| `shape` | `shape` (box, sphere, capsule, cylinder, plane), `size` [x, y, z] in metres, `color` |
| `model` | `asset` (glTF/GLB) |
| `sprite` | `asset`, optional `frame`, `size` [w, h], `flipX` |
| `collider` | `shape`, `size`, `body` (static, dynamic, kinematic), `trigger` |
| `light` | `kind` (directional, point, spot), `color`, `intensity`, `range` |
| `camera` | `fov` in degrees (3D) or `orthoSize` in metres (2D) |
| `sound` | `asset`, `volume`, `looped`, `autoplay` |
| `text` | `text`, `size`, `color` |
| `tiles` | `cell` [w, h] in metres, `sheets`, `cells` (see below) |
| `custom` | `name`, `values`: an engine component with no GameX equivalent yet, kept so nothing is lost |

A `tiles` part is a 2D tile grid. Each sheet is one image cut into tiles:

```json
{
  "type": "tiles",
  "cell": [0.16, 0.16],
  "sheets": [{ "asset": "textures/game/tiles.png", "tile": [16, 16], "margin": [0, 0], "separation": [1, 1], "solid": [[0, 0], [3, 2]] }],
  "cells": [[0, -1, 0, 3, 2, 0], [1, -1, 0, 0, 0, 1]]
}
```

- `tile` is one tile's size in pixels; `margin` and `separation` (pixels,
  optional) are the sheet's border and the gap between tiles.
- Each cell is `[x, y, sheet, column, row, flags]`: grid position (y up),
  which sheet, which tile in it, and `flags` 1 = flipped left-right,
  2 = flipped upside down, 4 = turned (swaps x and y).
- Cell (x, y)'s tile is drawn centred on ((x + 0.5) × cell w, (y + 0.5) ×
  cell h) at 100 pixels per metre; readers whose sprites use another
  density put the part on an entity scaled to match.
- `solid` lists the tiles [column, row] that block movement when played.

## Things

A reusable kind: what it's made of, its properties and behaviors. Scene
entities that name it get all of that.

```json
{
  "format": 1,
  "id": "guard",
  "name": "Guard",
  "kind": "character",
  "role": "enemy",
  "properties": [
    { "name": "health", "type": "number", "value": 50.0, "unit": "hp" },
    { "name": "chaseSpeed", "type": "number", "value": 3.5, "unit": "m/s" }
  ],
  "parts": [ { "type": "shape", "shape": "capsule", "size": [0.8, 1.8, 0.8], "color": "#c8483c" } ],
  "behaviors": [ { "behavior": "patrol-and-chase" } ]
}
```

`kind` is `character`, `item`, `tool`, `prop` or `other`, and decides the
folder. `role` (characters) is `player`, `enemy`, `npc` or `ally`.

## Behavior (blocks)

Rules of the form "when *event* [in *state*] and *conditions*, do
*actions*". A block's argument is a value, or one of the owner's properties
by name (`{"property": "chaseSpeed"}`), so changing the property changes
the behavior.

```json
{
  "format": 1,
  "id": "patrol-and-chase",
  "name": "Patrol and chase",
  "params": [ { "name": "giveUpDistance", "type": "number", "value": 12.0, "unit": "m" } ],
  "states": ["patrol", "chase"],
  "initialState": "patrol",
  "rules": [
    {
      "state": "patrol",
      "when": { "block": "everyFrame" },
      "if": [ { "block": "canSee", "args": {
        "target": { "type": "thing", "value": "player" },
        "distance": { "property": "sightRange" } } } ],
      "then": [ { "block": "setState", "args": { "state": { "type": "text", "value": "chase" } } } ]
    }
  ]
}
```

The block names form GameX's library, defined with the engine
(milestone 4). Version 1 stores and checks the structure: rules, states and
links to things.

## Origin

`sources\<game>\origin.json` maps element IDs to where they came from in
the original game, and how faithful the translation is:

```json
{
  "format": 1,
  "source": "granny",
  "entries": {
    "door-kitchen": { "file": "assets/bin/Data/level2", "object": "1834", "fidelity": "exact" },
    "granny-ai": { "file": "il2cpp/GrannyAI", "fidelity": "approximated", "note": "Rebuilt from field names and values" }
  }
}
```

`fidelity` is `exact`, `approximated` or `missing`.

Asset entries are keyed `asset:<path under assets/>` and name the game file
the asset came from. "Export modified APK" uses them to write replaced
assets back into the game; the first version of a replaced asset is kept in
`sources/replaced/<path under assets/>`.

## Undo

Every save through GameX first copies the file it replaces into `history\`
and logs it in `history\log.json`. Undo restores the newest change; a
change that touched several files (a thing moving folder) is undone as one.

## What GameX checks when it opens a project

Unreadable files, IDs that break the rules or repeat, entities naming a
thing that isn't there, parts using a missing asset, a missing behavior, a
rule using a state its behavior doesn't list, and a missing start scene.
Each problem names its file; the project still opens.
