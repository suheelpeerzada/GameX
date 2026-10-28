# GameX Plan: Translator and Engine

Agreed 2026-10-28. The living version, with diagrams and comments, is the
Claude Doc "GameX Plan: Translator and Engine"
(https://claude.ai/artifact/2ei3J2n2mqoTMSyXtMWPy1). This file is the copy
kept with the code.

## Vision and decisions

GameX translates any game into its own format, lets you edit the world, things and behavior visually, and plays the result in its own engine. The original game is never touched.

Decided on 2026-10-28:

- **Common layer:** GameX's own format, the same for every engine.
- **Engine:** GameX itself plays translated games, so different games can live in one project and nothing has to be added to an old engine.
- **First target:** the GameX engine and the translator. Rebuilding in the original engine is at most a later option.
- **Behavior is editable,** not only values. A translated game may play *similar* to the original, but not completely different.
- **Behavior editing:** visual blocks, with GameX scripts underneath for what blocks can't express.
- **Projects:** a folder of readable files in `Documents\GameX\Projects`, open to the user.
- **Never built for one test game:** one shared layer, one reader per engine, each reader tested on several games.
- **Way of working:** plan first; each piece of work is proposed and approved before it starts.

## How it fits together

One reader per engine translates games into the GameX format; the workspace and the engine only ever deal with that format, so they're written once for every game.

```
Unity games ─────┐
Godot games ─────┤                   ┌──────────────────────────────┐      ┌──────────────┐
2D-engine games ─┼─► Engine readers ─► GameX format                 ◄─────► Workspace     │ edit visually
Java/Kotlin games┘   one per engine  │  World: scenes and entities  │      └──────────────┘
                     originals       │  Things: characters, items   │      ┌──────────────┐
                     untouched       │  Behavior: blocks, scripts   ├──────► GameX engine  │ plays it
                                     │  Assets: models, sprites     │      └──────┬───────┘
                                     │  Origin: source of each part │      ┌──────▼───────┐
                                     └──────────────────────────────┘      │ New build    │ Windows, APK
                                                                           └──────────────┘
```

Games on the left are only read. Everything to the right of the readers is the same whatever engine a game came from.

## The GameX format

A GameX project is a folder of readable JSON files plus ordinary asset files, so it can be opened, searched and backed up like any other folder. Every engine is translated into the same five parts: world, things, behavior, assets and origin.

```
Documents\GameX\Projects\<Project name>\
  project.json               name, format version, the games translated into it, settings
  world\                     scenes: one file each
    <scene>.scene.json       a tree of entities with transforms, parts and behaviors
  things\                    what the world is made of, by kind
    characters\, items\, tools\, props\   <thing>.thing.json
  behavior\
    <name>.blocks.json       visual blocks
    <name>.gxs               GameX scripts
  assets\
    models\*.glb  textures\*.png  sprites\*.png + *.sprite.json
    animations\  audio\*.ogg|*.wav  fonts\
  sources\<game>\            one folder per translated game
    original\                untouched copy, read-only
    decompiled\              the readers' working files (can be rebuilt)
    origin.json              where every element came from
  history\                   undo
```

| Part | What it holds | Example |
| --- | --- | --- |
| Scene | A tree of entities: name, position, rotation, scale, parts, behaviors, tags | Level 2: the house, its rooms, doors and furniture |
| Entity part | Model, sprite, collider, light, camera, sound source, text or UI element | A door: a model, a box collider, a creak sound |
| Thing | A reusable kind with typed properties, default parts and behaviors; scene entities are instances that can override properties | Character "Enemy" with health 100, chase speed 7.1 |
| Behavior | Blocks or a script attached to a thing or entity, reading its properties | "When the player is within 5 m, chase at chase speed" |
| Asset | A model, texture, sprite sheet, animation, sound or font, in a common format | `assets\models\door.glb` |
| Origin | For each element: source game, file, object or field, and how sure the translation is | Door model = `sharedassets2.assets`, object 1834, exact |

- **One coordinate system:** right-handed, Y up, metres (the glTF convention). 2D scenes use the same axes with a pixels-per-unit setting, so 2D and 3D things can share a project.
- **Every element has a stable ID,** so edits, links between files and the origin map survive re-translation.
- **Combining games** means several folders under `sources\`; things from any of them can be placed in any scene.

## Behavior: blocks over scripts

Behavior is written as visual blocks wherever possible, with GameX scripts underneath for what blocks can't express. Both run on the same engine and read the same properties, so a block and a script can work on one character together.

**Blocks** read as sentences and are edited by picking from menus:

- **Events** start a rule: on start, every frame, on touch, when near, on input, on timer, on damage.
- **Conditions** check something: distance to, can see, property compare, random chance, state is.
- **Actions** do something: move to, chase, flee, patrol path, play animation, play sound, set property, spawn, destroy, change scene.
- **States** group rules into modes (idle, patrol, chase, attack), which is how most game AI is already structured.

Example: *When state is* `patrol` *and can see* `Player` *within* `chase range`*, set state to* `chase`.

**Scripts** (`.gxs`) are a small, readable language for the rest: custom math, unusual rules, data handling. Blocks show as script text too, so you can always look underneath. Each script can expose properties, and those appear in the inspector like any other.

**How behavior is edited in the workspace:**

1. Select a thing or entity; its behaviors are listed in the inspector.
2. Open one to see its blocks; change a value, a condition or an action from menus.
3. Switch to script view for anything blocks can't hold.
4. Press Play to try it in the GameX engine straight away.

The blocks library starts with what most games need, and grows as engine readers find patterns that recur across games.

## Translating behavior from each engine

How close a translated game plays depends on how readable its code is: readable code is translated logic by logic, compiled native code is rebuilt from its names, values and the blocks library. Every translated behavior is marked **exact**, **approximated** or **missing**, so you always know which parts to check.

| Game's code | Readable? | How GameX translates it | Expected result |
| --- | --- | --- | --- |
| Java/Kotlin (Android) | Yes, via jadx | Logic into scripts; recognised patterns into blocks | Close to the original |
| Godot GDScript | Yes, recovered by a decompiler | Nearly line for line; Godot calls mapped to GameX actions | Close to the original |
| Unity with C# DLLs (Mono, most PC games) | Yes, C# decompiles well | Like Java | Close to the original |
| Unity IL2CPP (most mobile Unity games) | No: native machine code | Class, field and method names plus saved values are recovered; behavior rebuilt from blocks that match them | Similar; needs checking |
| GameMaker | Yes, its bytecode decompiles | Like Java | Close to the original |
| RPG Maker MV/MZ | Yes: events are data, plugins are JavaScript | Events map almost one to one onto blocks | Very close |
| Ren'Py | Yes, scripts decompile | Dialogue, choices and flags map onto blocks | Very close |
| Native C/C++ games | No | Assets and values only; behavior rebuilt from blocks | Similar at best |

**The translation steps, the same for every engine:**

1. The engine reader turns the game's code into a neutral list of classes, fields, methods and the engine calls each one makes.
2. A mapping table per engine turns engine calls into GameX actions: move, physics, animation, sound, input, spawn.
3. Common patterns (state machines, chase and patrol, pickups, doors, health and damage) become blocks.
4. Everything else becomes script, kept readable and linked to the original method.
5. Optional: the AI suggests translations for unclear parts; you approve them. GameX works fully without AI.

**Keeping "similar":** GameX can run the original (on an emulator, as Run does today) beside the GameX version, with a checklist per thing: moves the same, reacts the same, same values. Anything that differs is flagged on that thing.

## The GameX engine

The GameX engine plays a GameX project directly, inside the app with one Play button, and later exports it as a standalone build. It's built in three stages, each usable on its own.

**Proposed technology:** the engine runs in the GameX window with the tools the app already uses: three.js for 3D and 2D drawing, Rapier for physics, and the browser's audio system. Editing and playing then happen in one place, with no second program to start. This is a decision for you (see Open questions).

| Stage | What you can do | What it includes |
| --- | --- | --- |
| 1. Walk the world | Play any translated scene and move through it | 3D and 2D rendering, cameras, lights, collisions and physics, animation playback, sounds; built-in controls (first person, third person, 2D platformer, top-down) |
| 2. Things act | Characters and items behave as in the original | The blocks and scripts runtime, states, events, UI (text, buttons, HUD), changing scenes, saving progress |
| 3. New builds and mixing | Combine games and share the result | Things from several games in one scene, with scale and units matched; export as a standalone Windows game, then an Android APK |

- **The new build** is a GameX project packed with the GameX engine. The original game's files are only read, never changed.
- **2D and 3D share one engine:** a 2D scene is a 3D scene seen straight on, so a project can mix both.
- **Play from any point:** start a scene with a chosen character, with the editor's changes live, and stop to return to editing.

## Engine readers

One reader per engine translates that engine's format into the GameX format, so it works for every game made with that engine. Proposed order: Godot first, because its scenes are text and its scripts are readable, which proves the whole chain fastest; then Unity, which covers the most games.

| Order | Engine | World | Things | Assets | Behavior |
| --- | --- | --- | --- | --- | --- |
| 1 | Godot 3 and 4 | Scenes (.tscn / .scn) | Scenes used as characters and items, their exported properties | Textures, models (glTF), audio, fonts | GDScript, recovered and translated |
| 2 | Unity 2019.3+ (Mono and IL2CPP) | Level files: GameObjects, transforms, components | Prefabs and their component values | Meshes, textures (ETC2/ASTC/DXT decoded), materials, sprites, audio, animations | C# for Mono; names and values for IL2CPP |
| 3 | RPG Maker MV/MZ | Maps (tile layers, events) | Actors, enemies, items, skills from its data files | Tilesets, sprites, audio (decrypted) | Event commands, plugins |
| 3 | Ren'Py | Scenes and backgrounds | Characters, flags | Images, audio | Dialogue, choices, conditions |
| 3 | GameMaker | Rooms, instances | Objects | Sprites, backgrounds, audio | GML, decompiled |
| 4 | Java/Kotlin Android games | Usually built in code, so recovered from it | Classes with positions and health | Images, sounds | Java, translated |

**How each reader is tested:**

- **Automated tests** with small generated files for every format version the reader claims to support.
- **At least three real games per engine,** of different kinds and engine versions, before the reader counts as done. Granny, the egg game and Draky are test files, not targets.
- **A report per game:** scenes, entities and assets found; behaviors exact, approximated and missing; anything left unread, with the reason.
- **A play check:** each test game's first scene plays in the GameX engine, compared side by side with the original.

## The workspace

The workspace shows a project the way a game editor does: the world in the middle, what's in it on the left, the selected thing's details on the right. It's the same for every engine, because it only ever shows the GameX format.

- **Left: project browser.** Scenes, things (characters, items, tools, props), behaviors and assets, grouped by kind and by source game. Engine plumbing (split files, caches, hex) never shows here.
- **Middle: scene view.** 3D or 2D, orbit and pan, click an object to select it, move, rotate and scale handles, snapping. A filter loads only what you pick (one room, one character) to keep large games fast.
- **Right: inspector.** Name, transform, parts (model, sprite, collider, sound…), properties with their units, and behaviors. Every field shows its origin and whether it's exact or approximated.
- **Bottom: behavior editor.** The selected thing's blocks, with a script view one click away.
- **Top: Play.** Runs the open scene in the GameX engine; Stop returns to editing with nothing lost.
- **Undo and history** for every change, across sessions; the original game is never changed.
- **Drag between games:** a thing from one translated game can be dragged into another game's scene.

Only the parts a project uses are loaded, as GameX already does for Live editing today: no 3D code for a 2D-only project, no behavior editor until it's opened.

## Milestones

The work runs in seven milestones; each ends at a gate that is checked with you before the next one starts. Milestone 1 comes first because every reader and the engine depend on the format.

| # | Milestone | What it covers | Gate |
| --- | --- | --- | --- |
| 1 | GameX format and workspace | Format v1, project folders, scene view, inspector, undo | A hand-made sample project opens, edits and saves |
| 2 | Godot reader | Scenes, things, assets; scripts translated as GameX scripts | 3 Godot games open in the workspace with nothing lost |
| 3 | GameX engine, stage 1 | Play scenes: rendering, physics, controls, animation, sound | Each Godot test game's first scene plays |
| 4 | Behavior blocks and scripts | Blocks editor, runtime, states; Godot behavior on blocks | Characters act similar to the original, side by side |
| 5 | Unity reader | Levels, prefabs, meshes, textures, materials; C# and IL2CPP | 3 Unity games open and their first scene plays |
| 6 | New builds and mixing games | Standalone Windows build, then APK; games combined in one scene | A new build runs on its own, outside GameX |
| 7 | More engines | RPG Maker, Ren'Py, GameMaker, then Java/Kotlin games | 3 games per engine pass the same checks |

Each milestone is proposed in detail and approved before work on it begins.

## What happens to today's GameX features

Most of today's GameX becomes the first step of the translator or a helper beside the workspace; nothing has to be thrown away.

| Feature today | What happens |
| --- | --- |
| Import (APK, ZIP, folder), untouched original, tool downloads | Kept: the first step of translating a game. Projects move to `Documents\GameX\Projects`. |
| Analyzer and categories (Characters, Map, Items…) | Becomes the first guess at which things are characters, items and tools. |
| Content tab (files, previews, hex) | Moves to a "Source files" view for checking the original; no longer the main view. |
| 3D model viewer, Godot scene editor | Reused inside the workspace. |
| Unity mesh reader | Becomes part of the Unity reader. |
| Run on an emulator | Kept: runs the original beside the GameX version to compare. |
| Live editing (Java games) | Kept: reads the original's live values, useful for comparing. |
| Build APK and "Keep this change" | Kept as is, not developed further: rebuilding the original isn't the target. |
| AI assistant | Kept, optional: helps translate unclear behavior. |
| Unity/Godot extraction (on hold) | Parts may be reused inside the Godot reader (GDRE's recovery); decided when that reader is designed. |

Existing projects in `%APPDATA%` can be moved to the new folder or imported again; that choice is in Open questions.

## Risks and open questions

The biggest risk is behavior from compiled native code (most mobile Unity games): it can only be rebuilt as *similar*, and how similar varies per game. Everything else in this plan uses known techniques.

**Risks**

- **IL2CPP and native games:** behavior comes from names, values and matching blocks, so some will be approximated or missing. Mitigation: the exact/approximated/missing marks and the side-by-side check.
- **Engine versions:** each engine changes its file formats over the years. Mitigation: readers state which versions they support and are tested on several.
- **Scale of the work:** a game engine, an editor and several readers is a long project. Mitigation: small milestones, each usable and checked before the next.
- **Performance with big games:** load on demand, cache translated parts, and keep lazy loading.
- **Sharing builds:** a new build made from someone else's game contains their assets. Fine for your own use and testing; sharing it publicly needs the rights to that game.

**Open questions**

- [ ] Engine technology: run in the GameX window with three.js and Rapier (proposed), or something else?
- [ ] Reader order: Godot first, then Unity (proposed), or Unity first?
- [ ] Test games: which games, at least three per engine, can we use? Free or open-source games could fill gaps.
- [ ] Existing projects in `%APPDATA%`: move them to `Documents\GameX\Projects`, or import again?
- [ ] Name for the format and the script language (working names: GameX project, `.gxs` scripts).
