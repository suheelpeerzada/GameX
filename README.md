# GameX

A Windows desktop app for importing offline games, recovering their content,
and editing it through one categorized editor. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

Status: **import, analysis, editing and rebuilding work** (milestones 1, 2,
4 and 5). You can import an APK, a ZIP, or a folder. GameX keeps a read-only
copy of the original, unpacks a working copy, detects the engine (Godot,
Unity, native Android, or generic for anything else) and sorts the content
into Characters, Core, Map, Items, Assets, Audio, UI, Code and Unsorted. You
can preview any object, move it to another category, edit smali and XML,
replace images and sounds (with undo), and rebuild the APK with a new name,
icon, package, version and permissions, signed with your own key.

## Run it on Windows

1. Install [Node.js](https://nodejs.org/) 20 or newer.
2. Install [Rust](https://rustup.rs/) with the MSVC toolchain. The installer
   offers the Visual Studio C++ Build Tools if they are missing.
3. WebView2 ships with Windows 10 and 11, so nothing else is needed there.
4. From the repo folder:

   ```
   npm install
   npm run tauri dev
   ```

To build an installer: `npm run tauri build` (output in
`src-tauri/target/release/bundle/`).

## Tools (optional, for APKs)

Without these, an APK import still works but only extracts the APK's files.

- [apktool](https://apktool.org/) decodes resources and smali, and rebuilds
  the APK. Needs Java.
- [jadx](https://github.com/skylot/jadx/releases) decompiles code to Java.
- [uber-apk-signer](https://github.com/patrickfav/uber-apk-signer/releases)
  aligns and signs rebuilt APKs. Needs Java 11 or newer, whose `keytool`
  creates your signing key.

Put them on PATH, or set their paths under **Settings** in the app
(`apktool.bat`, `jadx\bin\jadx.bat` and `uber-apk-signer.jar` on Windows).

## Editing and rebuilding

Open an object to edit its text files (smali, XML) or replace its images and
sounds. Code is changed in each class's smali file: the decompiled Java is
for reading only, because it can't be rebuilt reliably. Changes go to the
project's `edits\` folder, originals are never touched, and **Undo** reverts
the last change.

**Build APK** asks for the app name, icon, package, version and permissions,
then rebuilds and signs the game and saves the APK where you choose.

Builds are signed with your own key
(`%APPDATA%\com.gamex.app\signing\gamex-key.jks`, created on the first build;
back it up). Only a game's developer has its original key, so a rebuilt game
can't install over the original. Either uninstall the original first, or give
the build a different package name so both can be installed.

## Where imports go

`%APPDATA%\com.gamex.app\workspace\<project-id>\`:

```
original/       the imported file or folder, read-only, never modified
unpacked/       raw/ (APK files), apktool/, jadx/, or files/ for ZIPs and folders
edits/          your changed files, at the same paths as in unpacked/
history/        undo log and snapshots
build/          temporary, while building an APK
project.json    what was imported and how each unpacking step went
analysis.json   cached analysis
overrides.json  your manual category changes
```

## Development

```
npm run build                         # type-check and build the UI
cd src-tauri && cargo test            # import, analyzer, editing and build tests
```
