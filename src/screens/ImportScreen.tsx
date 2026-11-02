import { HelpCircle, Upload, Wrench } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { openHelp } from "../components/HelpPanel";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import ImportLoader from "../components/ImportLoader";
import ToolPrompt from "../components/ToolPrompt";
import { api, inDesktopApp, pickPath, type PickKind } from "../lib/api";
import type { ImportProgress, ImportRecord } from "../lib/types";
import { errorText } from "../lib/errors";
import ErrorText from "../components/ErrorText";

const SOURCES: { kind: PickKind; title: string; detail: string }[] = [
  {
    kind: "apk",
    title: "Android game (.apk)",
    detail: "The file you'd install on a phone. GameX opens a copy; your file is never changed.",
  },
  {
    kind: "zip",
    title: "Game in a .zip",
    detail: "A .zip file with a game's files inside.",
  },
  {
    kind: "folder",
    title: "Game folder",
    detail: "A folder with a game's files. GameX works on a copy, never the folder itself.",
  },
];

/** What Android games need, installed with one button from the welcome. */
const ANDROID_TOOLS = ["java", "apktool", "jadx", "uber-apk-signer"];

type Props = {
  /** No games yet: welcome first. */
  firstRun?: boolean;
  onImported: (record: ImportRecord) => void;
};

export default function ImportScreen({ firstRun = false, onImported }: Props) {
  // "Get tools for Android games": the ones still missing, while asking.
  const [getting, setGetting] = useState<string[] | null>(null);
  const [toolsReady, setToolsReady] = useState(false);
  const getTools = async () => {
    const missing = await api.toolsMissing(ANDROID_TOOLS).catch(() => ANDROID_TOOLS);
    if (missing.length) setGetting(missing);
    else setToolsReady(true);
  };
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
      setError(errorText(e));
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
      {getting && (
        <ToolPrompt
          missing={getting}
          action="Opening Android games"
          onReady={() => {
            setGetting(null);
            setToolsReady(true);
          }}
          onCancel={() => setGetting(null)}
        />
      )}
      {firstRun && (
        <section className="welcome">
          <h1>Welcome to GameX</h1>
          <p className="muted">
            Change the games you already have: how their characters look, their sounds, their levels and how hard they are. Then play your
            version, or make a new copy to install.
          </p>
          <div className="task-cards">
            <button className="task-card" disabled={busy !== null || !inDesktopApp} onClick={() => choose("apk")}>
              <Upload size={20} />
              <span className="task-title">Import a game</span>
              <span className="muted small">Pick a game's file, or drop it onto this window.</span>
            </button>
            <button className="task-card" disabled={!inDesktopApp || toolsReady} onClick={getTools}>
              <Wrench size={20} />
              <span className="task-title">{toolsReady ? "Android tools are ready" : "Get tools for Android games"}</span>
              <span className="muted small">One button installs what opening and rebuilding Android games needs (Java included).</span>
            </button>
            <button className="task-card" onClick={openHelp}>
              <HelpCircle size={20} />
              <span className="task-title">Learn the basics</span>
              <span className="muted small">Short how-tos and the keys, any time with F1.</span>
            </button>
          </div>
        </section>
      )}
      <h1>{firstRun ? "Import a game" : "Import another game"}</h1>
      <p className="muted">Pick what kind of file the game is, or drop it onto this window. GameX works on a copy, so your file is never changed.</p>

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
          <p><ErrorText text={error} /></p>
        </section>
      )}
    </div>
  );
}
