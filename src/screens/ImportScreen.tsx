import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import ImportLoader from "../components/ImportLoader";
import ToolPrompt from "../components/ToolPrompt";
import { api, inDesktopApp, pickPath, type PickKind } from "../lib/api";
import type { ImportProgress, ImportRecord } from "../lib/types";

const SOURCES: { kind: PickKind; title: string; detail: string }[] = [
  {
    kind: "apk",
    title: "Android APK",
    detail: "Unpacked with apktool and jadx when installed. The APK itself is never changed.",
  },
  {
    kind: "zip",
    title: "ZIP archive",
    detail: "A ZIP of game files or of an already decompiled project.",
  },
  {
    kind: "folder",
    title: "Folder",
    detail: "A game folder on disk. GameX works on a copy, never the folder itself.",
  },
];

type Props = { onImported: (record: ImportRecord) => void };

export default function ImportScreen({ onImported }: Props) {
  // What's being imported (its name) and the step it's on.
  const [busy, setBusy] = useState<string | null>(null);
  const [current, setCurrent] = useState<ImportProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const busyRef = useRef(false);
  // An APK waiting for tools the user is asked to install first.
  const [needs, setNeeds] = useState<{ path: string; missing: string[] } | null>(null);

  const run = async (path: string, checked = false) => {
    if (busyRef.current) return;
    if (!checked && inDesktopApp && path.toLowerCase().endsWith(".apk")) {
      const missing = await api.toolsMissing(["java", "apktool", "jadx"]);
      if (missing.length > 0) {
        setNeeds({ path, missing });
        return;
      }
    }
    busyRef.current = true;
    setBusy(path.split(/[\\/]/).filter(Boolean).pop()?.replace(/\.(apk|zip)$/i, "") ?? "Game");
    setError(null);
    setCurrent(null);
    const unlisten = await api.onImportProgress(setCurrent);
    try {
      onImported(await api.importGame(path));
    } catch (e) {
      setError(String(e));
    } finally {
      unlisten();
      busyRef.current = false;
      setBusy(null);
    }
  };

  // Dropping a file or folder anywhere on the window imports it.
  useEffect(() => {
    if (!inDesktopApp) return;
    const pending = getCurrentWebview().onDragDropEvent((event) => {
      const { type } = event.payload;
      if (type === "enter" || type === "over") setDragging(true);
      else setDragging(false);
      if (type === "drop" && event.payload.paths.length > 0) run(event.payload.paths[0]);
    });
    return () => {
      pending.then((unlisten) => unlisten());
    };
  }, []);

  const choose = async (kind: PickKind) => {
    const path = await pickPath(kind);
    if (path) run(path);
  };

  return (
    <div className="screen">
      {needs && (
        <ToolPrompt
          missing={needs.missing}
          action="Importing this APK"
          without="the APK is only extracted: no decoded resources or readable code"
          onReady={() => {
            const path = needs.path;
            setNeeds(null);
            run(path, true);
          }}
          onSkip={() => {
            const path = needs.path;
            setNeeds(null);
            run(path, true);
          }}
          onCancel={() => setNeeds(null)}
        />
      )}
      <h1>Import a game</h1>
      <p className="muted">
        Pick how the game is packaged, or drop it onto this window. GameX keeps the original
        untouched and unpacks a working copy.
      </p>

      <div className={dragging ? "sources dragging" : "sources"}>
        {SOURCES.map((s) => (
          <button
            key={s.kind}
            className="source-card"
            disabled={busy !== null || !inDesktopApp}
            onClick={() => choose(s.kind)}
          >
            <span className="source-title">{s.title}</span>
            <span className="muted small">{s.detail}</span>
          </button>
        ))}
      </div>

      {busy !== null && <ImportLoader name={busy} current={current} />}

      {error && (
        <section className="panel error">
          <h2>Import failed</h2>
          <p>{error}</p>
        </section>
      )}
    </div>
  );
}
