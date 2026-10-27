import { Play, RefreshCw, Smartphone, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import type { RunOptions, RunStatus, RunTarget } from "../lib/types";

interface Props {
  projectId: string;
  open: boolean;
  onClose: () => void;
  /** Reports whether the game is running, for the header button. */
  onRunning: (running: boolean) => void;
  onOpenSettings: () => void;
}

type Problem = { kind: "adb"; index: number } | { kind: "replace" } | { kind: "error"; message: string } | null;

/**
 * Runs the game on an emulator or phone: pick a target and an APK, then
 * GameX starts the emulator, installs, launches and keeps an eye on it.
 * Stays mounted while closed so it can keep watching the game.
 */
export default function RunDialog({ projectId, open, onClose, onRunning, onOpenSettings }: Props) {
  const [targets, setTargets] = useState<RunTarget[] | null>(null);
  const [target, setTarget] = useState<string>("");
  const [options, setOptions] = useState<RunOptions | null>(null);
  const [apk, setApk] = useState<"original" | "build">("original");
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const [problem, setProblem] = useState<Problem>(null);
  const [status, setStatus] = useState<RunStatus | null>(null);
  const wasRunning = useRef(false);

  const refresh = useCallback(async () => {
    const list = await api.runTargets().catch(() => []);
    setTargets(list);
    setTarget((t) => t || list.find((x) => x.running)?.id || list[0]?.id || "");
    setOptions(await api.runOptions(projectId).catch(() => null));
  }, [projectId]);

  useEffect(() => {
    if (open && targets === null) refresh();
  }, [open, targets, refresh]);

  // Watch the game every 2 seconds once one has been started.
  useEffect(() => {
    let timer: number | undefined;
    const tick = async () => {
      const s = await api.runStatus(projectId).catch(() => null);
      setStatus(s);
      const running = !!s?.status.running;
      onRunning(running);
      wasRunning.current = running;
      timer = window.setTimeout(tick, 2000);
    };
    tick();
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  const start = async (replace = false) => {
    setBusy(true);
    setProblem(null);
    setLog([]);
    const unlisten = await api.onRunProgress((p) => setLog((l) => [...l, p.message]));
    try {
      await api.runGame(projectId, target, apk, replace);
      setLog((l) => [...l, "The game is running."]);
      setStatus(await api.runStatus(projectId).catch(() => null));
    } catch (e) {
      const message = String(e);
      if (message.includes("ADB_DISABLED")) setProblem({ kind: "adb", index: Number(target.replace("ld:", "")) });
      else if (message.includes("REPLACE")) setProblem({ kind: "replace" });
      else setProblem({ kind: "error", message });
    } finally {
      unlisten();
      setBusy(false);
    }
  };

  const enableAdb = async (index: number) => {
    setBusy(true);
    setProblem(null);
    setLog(["Turning on ADB debugging (LDPlayer restarts)"]);
    try {
      await api.runEnableAdb(index);
      await refresh();
    } catch (e) {
      setProblem({ kind: "error", message: String(e) });
      setBusy(false);
      return;
    }
    setBusy(false);
    start();
  };

  const stop = async () => {
    await api.runStop(projectId).catch(() => {});
    setStatus(await api.runStatus(projectId).catch(() => null));
  };

  if (!open) return null;
  const current = targets?.find((t) => t.id === target);
  const running = status?.status.running;
  const stopped = status && !status.status.running;
  const chosenApk = apk === "build" ? options?.lastBuild : options?.original;

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal run-dialog" role="dialog" aria-modal="true">
        <h2>
          <Play size={18} /> Run the game
        </h2>

        {status && (
          <div className={running ? "run-status ok" : "run-status stopped"}>
            <span className={running ? "status-dot ok" : "status-dot bad"} />
            <div>
              <strong>{running ? "Running" : "The game stopped"}</strong>{" "}
              <span className="muted small">
                {status.launched.package} on {targets?.find((t) => t.id === status.launched.target)?.name ?? status.launched.target}
                {running && status.status.pid ? ` · process ${status.status.pid}` : ""}
              </span>
              {stopped && status.status.crashLog && (
                <>
                  <div className="small">It crashed. Android's crash report:</div>
                  <pre className="code run-crash">{status.status.crashLog}</pre>
                </>
              )}
              {stopped && !status.status.crashLog && (
                <div className="muted small">It was closed (or stopped from GameX); no crash was reported.</div>
              )}
            </div>
            <div className="run-status-actions">
              {running ? (
                <button onClick={stop}>
                  <Square size={14} /> Stop
                </button>
              ) : (
                <button className="primary" onClick={() => start()} disabled={busy}>
                  <RefreshCw size={14} /> Restart
                </button>
              )}
            </div>
          </div>
        )}

        {targets !== null && targets.length === 0 ? (
          <div className="panel">
            <p>No emulator or phone was found.</p>
            <p className="muted small">
              Install LDPlayer (or Google's Android Emulator), or connect a phone with USB debugging and install Android
              platform tools. All are under Settings → Tools.
            </p>
            <div className="row">
              <button className="primary" onClick={onOpenSettings}>
                Open Settings → Tools
              </button>
              <button onClick={refresh}>Look again</button>
            </div>
          </div>
        ) : (
          <div className="form">
            <label className="field">
              <span className="label">Run on</span>
              <div className="row">
                <select value={target} onChange={(e) => setTarget(e.target.value)} disabled={busy}>
                  {targets === null && <option>Looking for emulators…</option>}
                  {targets?.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name} ({t.kind === "ldplayer" ? "LDPlayer" : "device"}
                      {t.running ? ", running" : ""})
                    </option>
                  ))}
                </select>
                <button onClick={refresh} disabled={busy} title="Look for emulators and devices again">
                  <RefreshCw size={14} />
                </button>
              </div>
              {current?.kind === "ldplayer" && current.adbEnabled === false && (
                <span className="muted small">
                  ADB debugging is off for this instance; GameX will offer to turn it on.
                </span>
              )}
            </label>

            <div className="field">
              <span className="label">APK</span>
              <label className="radio">
                <input type="radio" checked={apk === "original"} onChange={() => setApk("original")} disabled={busy} />
                Original game {options?.original && <span className="muted small">({options.original.package})</span>}
              </label>
              <label className="radio">
                <input
                  type="radio"
                  checked={apk === "build"}
                  onChange={() => setApk("build")}
                  disabled={busy || !options?.lastBuild}
                />
                Your latest build{" "}
                {options?.lastBuild ? (
                  <span className="muted small">
                    ({options.lastBuild.package}, {options.lastBuild.path.split(/[\\/]/).pop()})
                  </span>
                ) : (
                  <span className="muted small">(none yet; use Build APK)</span>
                )}
              </label>
            </div>
          </div>
        )}

        {log.length > 0 && (
          <ol className="log">
            {log.map((m, i) => (
              <li key={i} className={i === log.length - 1 && busy ? "current" : undefined}>
                {m}
              </li>
            ))}
          </ol>
        )}

        {problem?.kind === "adb" && (
          <div className="panel">
            <p>
              <strong>ADB debugging is off in this LDPlayer instance.</strong> GameX needs it to install the game and watch
              it. Turning it on changes that one LDPlayer setting and restarts the instance.
            </p>
            <button className="primary" onClick={() => enableAdb(problem.index)} disabled={busy}>
              Turn it on and continue
            </button>
          </div>
        )}
        {problem?.kind === "replace" && (
          <div className="panel">
            <p>
              <strong>A different copy of this game is already installed</strong> (another version, or one signed with a
              different key). It has to be removed first, which also deletes its saved progress on this emulator.
            </p>
            <button className="primary" onClick={() => start(true)} disabled={busy}>
              Remove it and install
            </button>
          </div>
        )}
        {problem?.kind === "error" && <div className="panel error small">{problem.message}</div>}

        <div className="modal-actions">
          <span className="muted tiny run-note">
            <Smartphone size={12} /> The game runs in the emulator, separate from GameX: closing this window keeps it
            running.
          </span>
          <button onClick={onClose} disabled={busy}>
            Close
          </button>
          <button className="primary" onClick={() => start()} disabled={busy || !target || !chosenApk}>
            <Play size={14} />
            {busy ? "Working…" : running ? "Run again" : "Run"}
          </button>
        </div>
      </div>
    </div>
  );
}
