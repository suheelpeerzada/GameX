import { useEffect, useState } from "react";
import { api, formatBytes, pickFile, pickSaveApk } from "../lib/api";
import ToolPrompt from "./ToolPrompt";
import type { BuildInfo, BuildResult } from "../lib/types";
import { errorText } from "../lib/errors";
import ErrorText from "./ErrorText";

interface Props {
  projectId: string;
  editedCount: number;
  onClose: () => void;
}

type Stage =
  | { kind: "form" }
  | { kind: "building"; log: string[] }
  | { kind: "done"; result: BuildResult }
  | { kind: "failed"; error: string };

const PACKAGE = /^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)+$/;

/** What a permission lets the game do, in words. */
const PERMISSION_WORDS: Record<string, string> = {
  INTERNET: "Go online",
  ACCESS_NETWORK_STATE: "See if the phone is online",
  ACCESS_WIFI_STATE: "See Wi-Fi status",
  VIBRATE: "Vibrate",
  WAKE_LOCK: "Keep the screen on",
  WRITE_EXTERNAL_STORAGE: "Save files on the phone",
  READ_EXTERNAL_STORAGE: "Read files on the phone",
  READ_MEDIA_IMAGES: "See your photos",
  READ_MEDIA_AUDIO: "See your music",
  RECORD_AUDIO: "Use the microphone",
  CAMERA: "Use the camera",
  ACCESS_FINE_LOCATION: "Know exactly where you are",
  ACCESS_COARSE_LOCATION: "Know roughly where you are",
  POST_NOTIFICATIONS: "Show notifications",
  RECEIVE_BOOT_COMPLETED: "Start when the phone starts",
  FOREGROUND_SERVICE: "Keep running in the background",
  BILLING: "In-app purchases",
  AD_ID: "Use the advertising ID (ads)",
  READ_PHONE_STATE: "Read phone details",
  GET_ACCOUNTS: "See your accounts",
  BLUETOOTH: "Use Bluetooth",
  BLUETOOTH_CONNECT: "Connect to Bluetooth devices",
};

function permissionWords(p: string): string {
  const name = p.split(".").pop() ?? p;
  return PERMISSION_WORDS[name] ?? name.replace(/_/g, " ").toLowerCase().replace(/^./, (c) => c.toUpperCase());
}

/** Asks for the app's name, icon, package, version and permissions, then builds and saves the APK. */
export default function BuildDialog({ projectId, editedCount, onClose }: Props) {
  const [info, setInfo] = useState<BuildInfo | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [appName, setAppName] = useState("");
  const [pkg, setPkg] = useState("");
  const [versionName, setVersionName] = useState("");
  const [versionCode, setVersionCode] = useState("");
  const [iconPath, setIconPath] = useState<string | null>(null);
  const [iconPreview, setIconPreview] = useState<string | null>(null);
  const [removed, setRemoved] = useState<Set<string>>(new Set());
  const [stage, setStage] = useState<Stage>({ kind: "form" });
  const [missingTools, setMissingTools] = useState<string[]>([]);
  const [prompting, setPrompting] = useState(false);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    api.toolsMissing(["java", "apktool", "uber-apk-signer"]).then(setMissingTools).catch(() => {});
    api
      .getBuildInfo(projectId)
      .then((i) => {
        setInfo(i);
        setAppName(i.appName);
        // Next to the original by default: a rebuilt game can't install over it anyway.
        setPkg(`${i.package}.mod`);
        setVersionName(i.versionName);
        setVersionCode(String(i.versionCode));
      })
      .catch((e) => setLoadError(errorText(e)));
  }, [projectId, reload]);

  const building = stage.kind === "building";
  // Esc closes the dialog, except while a build is running.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !building && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [building, onClose]);

  const code = Number(versionCode);
  const problems: string[] = [];
  if (!appName.trim()) problems.push("Enter an app name.");
  if (!PACKAGE.test(pkg)) problems.push("The package name must look like com.example.game.");
  if (!Number.isInteger(code) || code < 1) problems.push("The version code must be a whole number of 1 or more.");
  if (!versionName.trim()) problems.push("Enter a version name, like 1.0.");

  const chooseIcon = async () => {
    const path = await pickFile("PNG image", ["png"]);
    if (!path) return;
    try {
      setIconPreview(await api.readPng(path));
      setIconPath(path);
    } catch (e) {
      window.alert(errorText(e));
    }
  };

  const build = async () => {
    const fileName = `${appName.trim().replace(/[^\w.-]+/g, "_") || "game"}-${versionName.trim()}.apk`;
    const outputPath = await pickSaveApk(fileName);
    if (!outputPath) return;
    const log: string[] = [];
    setStage({ kind: "building", log });
    const unlisten = await api.onBuildProgress((p) => {
      log.push(p.message);
      setStage({ kind: "building", log: [...log] });
    });
    try {
      const result = await api.buildApk(projectId, {
        appName: appName.trim(),
        package: pkg.trim(),
        versionName: versionName.trim(),
        versionCode: code,
        iconPath,
        removedPermissions: [...removed],
        outputPath,
      });
      setStage({ kind: "done", result });
    } catch (e) {
      setStage({ kind: "failed", error: errorText(e) });
    } finally {
      unlisten();
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !building && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="build-title">
        <h2 id="build-title">Make a new APK</h2>

        {loadError && <div className="panel error"><ErrorText text={loadError} /></div>}
        {!info && !loadError && <p className="muted">Reading the game's settings…</p>}

        {info && stage.kind === "form" && (
          <>
            {prompting && (
              <ToolPrompt
                missing={missingTools}
                action="Building an APK"
                onReady={() => {
                  setPrompting(false);
                  setReload((r) => r + 1);
                }}
                onCancel={() => setPrompting(false)}
              />
            )}
            {info.problems.length > 0 && (
              <div className="panel error">
                <h2>Can't build yet</h2>
                {missingTools.length > 0 && (
                  <button className="primary" onClick={() => setPrompting(true)}>
                    Install the missing tools
                  </button>
                )}
                <ul className="evidence">
                  {info.problems.map((p) => (
                    <li key={p}>{p}</li>
                  ))}
                </ul>
              </div>
            )}

            <p className="muted small">
              {editedCount === 0
                ? "You haven't changed any files, so this rebuilds the game as it is, with the settings below."
                : `${editedCount} edited file${editedCount === 1 ? "" : "s"} will be included.`}
            </p>

            <div className="form">
              <div className="field icon-field">
                <span className="label">Icon</span>
                <div className="row">
                  <div className="icon-box">
                    {iconPreview || info.icon ? <img src={iconPreview ?? info.icon ?? ""} alt="" /> : <span className="muted small">none</span>}
                  </div>
                  <button onClick={chooseIcon}>Choose PNG…</button>
                  {iconPath && (
                    <button
                      onClick={() => {
                        setIconPath(null);
                        setIconPreview(null);
                      }}
                    >
                      Keep original
                    </button>
                  )}
                </div>
              </div>

              <label className="field">
                <span className="label">App name</span>
                <input type="text" value={appName} onChange={(e) => setAppName(e.target.value)} />
                <span className="muted small">Shown under the icon on the phone.</span>
              </label>

              <label className="field switch-field">
                <input type="checkbox" checked={pkg !== info.package} onChange={(e) => setPkg(e.target.checked ? `${info.package}.mod` : info.package)} />
                <span>
                  <span className="label">Install next to the original game</span>
                  <span className="muted small">
                    {pkg !== info.package
                      ? "Both games can be on the phone at once."
                      : "Replaces the original: uninstall the original game first (its signature can't be reused)."}
                  </span>
                </span>
              </label>

              <div className="field">
                <span className="label">What the game may do</span>
                {info.permissions.length === 0 ? (
                  <span className="muted small">This game asks for no permissions.</span>
                ) : (
                  <>
                    <span className="muted small">Untick anything you don't want it to do. Without "Go online" the game stays fully offline.</span>
                    <ul className="permissions">
                      {info.permissions.map((p) => (
                        <li key={p}>
                          <label title={p}>
                            <input
                              type="checkbox"
                              checked={!removed.has(p)}
                              onChange={(e) =>
                                setRemoved((prev) => {
                                  const next = new Set(prev);
                                  if (e.target.checked) next.delete(p);
                                  else next.add(p);
                                  return next;
                                })
                              }
                            />
                            <span>{permissionWords(p)}</span>
                          </label>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </div>

              <details className="advanced">
                <summary>Advanced</summary>
                <label className="field">
                  <span className="label">Package name</span>
                  <input type="text" className="mono" value={pkg} onChange={(e) => setPkg(e.target.value)} />
                  <span className="muted small">The game's ID on the phone. Original: {info.package}</span>
                </label>
                <div className="field-pair">
                  <label className="field">
                    <span className="label">Version name</span>
                    <input type="text" value={versionName} onChange={(e) => setVersionName(e.target.value)} />
                    <span className="muted small">Shown to people, e.g. 1.0-mod. Original: {info.versionName || "none"}</span>
                  </label>
                  <label className="field">
                    <span className="label">Version code</span>
                    <input type="number" min={1} value={versionCode} onChange={(e) => setVersionCode(e.target.value)} />
                    <span className="muted small">A whole number; higher counts as newer. Original: {info.versionCode}</span>
                  </label>
                </div>
                <span className="muted small">To change anything else in the manifest, edit AndroidManifest.xml under Files.</span>
              <div className="field">
                <span className="label">Signing</span>
                <span className="small">
                  Signed with your own GameX key. The game's original signature can't be reused: only its developer
                  has that private key.
                </span>
                <span className="muted small">
                  {info.keyExists
                    ? "Your key: "
                    : "A key will be created on the first build and saved at "}
                  <span className="mono">{info.keyPath}</span>. Back it up: updates to this build must be signed with the
                  same key.
                </span>
              </div>
              </details>
            </div>

            {problems.length > 0 && <div className="muted small form-problems">{problems.join(" ")}</div>}

            <div className="modal-actions">
              <button onClick={onClose}>Cancel</button>
              <button
                className="primary"
                disabled={problems.length > 0 || info.problems.length > 0}
                onClick={build}
              >
                Build and save…
              </button>
            </div>
          </>
        )}

        {stage.kind === "building" && (
          <>
            <ol className="log">
              {stage.log.map((m, i) => (
                <li key={i} className={i === stage.log.length - 1 ? "current" : undefined}>
                  {m}
                </li>
              ))}
            </ol>
            <p className="muted small">Building takes about a minute. Keep GameX open.</p>
          </>
        )}

        {stage.kind === "done" && (
          <>
            <p>
              <strong>Saved.</strong> Your APK is ready.
            </p>
            <dl className="details">
              <dt>File</dt>
              <dd className="mono">{stage.result.path}</dd>
              <dt>Size</dt>
              <dd>{formatBytes(stage.result.size)}</dd>
            </dl>
            <p className="muted small">
              To install it, copy the file to your phone and open it (allow installs from this source if asked), or use Run original to
              try it on an emulator.
              {stage.result.package === info?.package && " Uninstall the original game first: this copy replaces it."}
            </p>
            <details className="advanced">
              <summary>Advanced</summary>
              <dl className="details">
                <dt>Package</dt>
                <dd className="mono">{stage.result.package}</dd>
                <dt>SHA-256</dt>
                <dd className="mono">{stage.result.sha256}</dd>
                <dt>Install with adb</dt>
                <dd className="mono">adb install "{stage.result.path}"</dd>
              </dl>
            </details>
            <div className="modal-actions">
              <button className="primary" onClick={onClose}>
                Done
              </button>
            </div>
          </>
        )}

        {stage.kind === "failed" && (
          <>
            <div className="panel error">
              <h2>The build failed</h2>
              <p><ErrorText text={stage.error} /></p>
            </div>
            <p className="muted small">
              Your edits are safe. If you changed smali or XML, the message above usually names the file and line.
            </p>
            <div className="modal-actions">
              <button onClick={onClose}>Close</button>
              <button className="primary" onClick={() => setStage({ kind: "form" })}>
                Back to settings
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
