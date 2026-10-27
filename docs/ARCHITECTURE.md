# GameX architecture

GameX is a desktop app (Windows first) for importing offline games, recovering
their content, and presenting it through one consistent, categorized editor.
This note fixes the shape of the system before features are built.

## Stack

- **Shell:** Tauri 2. The UI is React + TypeScript; the backend is Rust.
- **Heavy lifting in Rust:** archive extraction, file hashing, analysis, and
  calling external tools all run in the backend, never in the webview.
- **External tools** are invoked as child processes, not linked in:
  apktool (APK resources + smali), jadx (readable Java), and later
  engine-specific tools (e.g. gdsdecomp for Godot, AssetRipper for Unity).
  Paths are configured in settings; GameX reports clearly when a tool is missing.

## Components

1. **Game Analyzer** (`src-tauri/src/analyzer/`): turns an imported package
   into a `GameProject`, plus a `CapabilityReport` saying what was and wasn't
   recovered.
2. **Universal Editor** (`src/`): the UI. It only ever talks to the common
   `GameProject` model, never to engine-specific formats.
3. **AI Extension** (later): an optional panel behind an `AiProvider`
   interface. Claude via the Anthropic API is the first provider. The app
   must work fully without it.
4. **Game Projection** (later): a separate module with its own process model.
   Nothing in phases 1 to 3 depends on it.

## Two layers: containers and engines

A game arrives inside a **container** and is built with an **engine**. They
are detected separately because the same engine ships in many containers.

| Layer     | Examples                                   | Job                                 |
|-----------|--------------------------------------------|-------------------------------------|
| Container | APK, ZIP of decompiled files, plain folder, Windows exe | Unpack safely into the workspace |
| Engine    | Godot, Unity, native Android (Java/Kotlin), GameMaker   | Understand and categorize content |

Flow: `import` → container unpacks → each engine integration's `detect()`
scores the unpacked tree → the best match runs `analyze()` → the result is a
`GameProject`. If no engine matches, a generic integration still lists every
file by type so nothing is silently dropped.

## Integration interface

Every engine integration implements the same Rust trait
(`src-tauri/src/analyzer/mod.rs`):

```rust
trait EngineIntegration {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Score 0.0 = definitely not this engine, 1.0 = certain, plus evidence.
    fn detect(&self, tree: &FileTree) -> Detection;
    fn analyze(&self, tree: &FileTree, ctx: &Context) -> Vec<GameObject>;
    fn capabilities(&self, tree: &FileTree, ctx: &Context, objects: &[GameObject]) -> Vec<Capability>;
    // Later phases, optional per engine:
    // fn apply_edit(...), fn rebuild(...), fn live_preview(...)
}
```

Integrations today: Godot, Unity, Native Android (Java/Kotlin) and Generic.
The pipeline assembles the `GameProject` from what the chosen integration
returns. Unsupported operations return "not supported" instead of pretending.

## How classification stays universal

GameX must work for any game, so classification never depends on one game's
names. It has two layers:

1. **Generic classifier** (`analyzer/classify.rs`), used for every game:
   - File type first (images, audio, fonts, models, code, map formats...).
   - Then general game vocabulary in the file name and its folders
     ("enemy", "coin", "tileset", "button"...). Specific categories beat
     Core, whole words beat prefixes, and the last word wins
     (`map_icon` is an icon).
   - Structural patterns: numbered images (`walk1`, `walk2`) become one
     image set; a sprite drawn facing three or more directions
     (`_u`, `_d`, `_left`...) is treated as a character.
   - Anything with no signal goes to Unsorted.
2. **Engine integrations** first claim the files they know for certain
   (a Godot `.pck`, Unity `levelN` files, Android resources and code
   classes) and leave every other file to the generic classifier.

Every object records a confidence and a plain-language reason. Every file in
`unpacked/` belongs to exactly one object; tests check this for each kind of
game. Copies of the same file from different unpack steps (raw, apktool,
jadx) are merged into one object, best-decoded copy first.

## Common model

- `GameProject`: id, name, container kind, engine id, workspace path,
  objects, capability report.
- `GameObject`: id, category, display name, kind (character, sprite, script,
  map, sound...), source files, known properties.
- `Category`: Characters, Core, Map, Items, Assets, Audio, UI, Code, plus
  **Unsorted** for anything the analyzer couldn't place.
- `CapabilityReport`: one entry per feature (code, characters, movement,
  assets, visual editing, rebuilding) with a status of
  `Supported | Partial | Unsupported | Unknown` and a plain-language note.

## Workspace layout

Each import gets its own folder under the app data directory:

```
<workspace>/<project-id>/
  original/      # the imported file, byte-for-byte, never modified
  unpacked/      # container output (e.g. apktool + jadx result)
  edits/         # changed files only, at the same path as in unpacked/
  history/       # undo log (log.json) + snapshots of replaced edits
  build/         # temporary work copy while building an APK
  project.json   # what was imported and how each step went
  analysis.json  # cached GameProject (redone when the analyzer changes)
  overrides.json # the user's manual category changes (never redone)
```

## APK specifics

- Unpack with apktool (`apktool d`) and jadx (`jadx -d`) into `unpacked/`.
- Engine detection inside the APK: `lib/*/libgodot_android.so` or `*.pck`
  → Godot; `libunity.so` / `assets/bin/Data` → Unity; otherwise native.
- Editing (`src-tauri/src/edits.rs`): only apktool's output (smali,
  resources, manifest) is editable, because that is what gets rebuilt.
  jadx's Java is a read-only view; decompiled Java can't be recompiled
  reliably. Edits go to `edits/`, never to `unpacked/`, and every change
  (edit, replace, restore) is logged so it can be undone.
- Rebuilding (`src-tauri/src/rebuild.rs`): copy apktool's output to
  `build/work`, overlay the edits, apply the build dialog's choices (name,
  icon, package, version, permissions), `apktool b`, then zipalign + sign
  with uber-apk-signer (v1/v2/v3 signatures).
- Signing: a game can't be re-signed with its developer's key, since an
  APK only carries the public certificate. GameX creates one key per user
  with keytool on the first build (`<app data>/signing/gamex-key.jks`).
  A rebuilt game therefore can't install over the original. To install
  both side by side, change the package name in the build dialog.
- Package renaming: apktool 3 dropped `renameManifestPackage`, so GameX
  edits the manifest. It first makes relative class names absolute, then
  renames provider authorities and custom permissions that start with the
  old package.
- Scope: offline single-player games only.

## Rules (from the roadmap)

1. AI is never required for basic functionality.
2. Decompiled does not mean editable; the capability report says which.
3. Originals are preserved and every change is reversible.
4. Unrecognized content goes to Unsorted, never discarded.
5. Users can manually correct any automatic classification.
6. New engines are added as integrations, not special cases in the UI.
7. Each milestone is tested before scope expands.

## First target

Draky, as an APK, through: Import → Analyze → Categorize → Inspect → Ask AI.
Draky is a test case only: nothing in the analyzer is specific to it.
