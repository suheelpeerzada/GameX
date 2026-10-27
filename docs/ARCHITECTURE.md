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

Every engine integration implements the same Rust trait:

```rust
trait EngineIntegration {
    fn id(&self) -> &'static str;
    /// 0.0 = definitely not this engine, 1.0 = certain.
    fn detect(&self, tree: &FileTree) -> f32;
    fn analyze(&self, tree: &FileTree) -> Result<GameProject, AnalyzeError>;
    fn capabilities(&self, project: &GameProject) -> CapabilityReport;
    // Later phases, optional per engine:
    // fn apply_edit(...), fn rebuild(...), fn live_preview(...)
}
```

Unsupported operations return "not supported" instead of pretending.

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
  edits/         # changed files only (phase 3)
  history/       # snapshots for undo (phase 2+)
  project.json   # cached GameProject
```

## APK specifics

- Unpack with apktool (`apktool d`) and jadx (`jadx -d`) into `unpacked/`.
- Engine detection inside the APK: `lib/*/libgodot_android.so` or `*.pck`
  → Godot; `libunity.so` / `assets/bin/Data` → Unity; otherwise native.
- Rebuilding (phase 3): `apktool b` → `zipalign` → `apksigner` with a
  user-owned debug key. The user's key is created once and stored locally.
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
