# GameX

A Windows desktop app for importing offline games, recovering their content,
and editing it through one categorized editor. See
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how it fits together.

Status: **milestone 1, app shell and import.** You can import an APK, a ZIP,
or a folder. GameX keeps a read-only copy of the original and unpacks a
working copy. Engine detection and the categorized view come next.

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

## Decompilers (optional, for APKs)

Without these, an APK import still works but only extracts the APK's files.

- [apktool](https://apktool.org/) decodes resources and smali. Needs Java.
- [jadx](https://github.com/skylot/jadx/releases) decompiles code to Java.

Put them on PATH, or set their paths under **Settings** in the app
(`apktool.bat` and `jadx\bin\jadx.bat` on Windows).

## Where imports go

`%APPDATA%\com.gamex.app\workspace\<project-id>\`:

```
original/      the imported file or folder, read-only, never modified
unpacked/      raw/ (APK files), apktool/, jadx/, or files/ for ZIPs and folders
edits/         changed files only (later)
history/       undo snapshots (later)
project.json   what was imported and how each unpacking step went
```

## Development

```
npm run build                         # type-check and build the UI
cd src-tauri && cargo test            # import tests (APK, ZIP, folder, zip-slip)
```
