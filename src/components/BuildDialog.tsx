import { useEffect, useState } from "react";
import { api, formatBytes, pickFile, pickSaveApk } from "../lib/api";
import ToolPrompt from "./ToolPrompt";
import type { BuildInfo, BuildResult } from "../lib/types";

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
        setPkg(i.package);
        setVersionName(i.versionName);
        setVersionCode(String(i.versionCode));
      })
      .catch((e) => setLoadError(String(e)));
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
      window.alert(String(e));
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
      setStage({ kind: "failed", error: String(e) });
    } finally {
      unlisten();
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !building && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="build-title">
        <h2 id="build-title">Build APK</h2>

        {loadError && <div className="panel error">{loadError}</div>}
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

              <label className="field">
                <span className="label">Package name</span>
                <input type="text" className="mono" value={pkg} onChange={(e) => setPkg(e.target.value)} />
                <span className="muted small">
                  {pkg === info.package ? (
                    <>
                      Same as the original. Android won't install this over the original game because it's signed
                      with a different key, so uninstall the original first.{" "}
                      <button className="link" onClick={() => setPkg(`${info.package}.mod`)}>
                        Use {info.package}.mod instead
                      </button>
                    </>
                  ) : (
                    <>Different from the original ({info.package}), so both can be installed side by side.</>
                  )}
                </span>
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

              <div className="field">
                <span className="label">Permissions (from the manifest)</span>
                {info.permissions.length === 0 ? (
                  <span className="muted small">This game asks for no permissions.</span>
                ) : (
                  <>
                    <span className="muted small">Untick a permission to remove it. Removing INTERNET keeps the game fully offline.</span>
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
                            <span className="mono">{p.replace(/^android\.permission\./, "")}</span>
                          </label>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
                <span className="muted small">
                  To change anything else in the manifest, edit AndroidManifest.xml under Core.
                </span>
              </div>

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
              <dt>Package</dt>
              <dd className="mono">{stage.result.package}</dd>
              <dt>SHA-256</dt>
              <dd className="mono">{stage.result.sha256}</dd>
            </dl>
            <p className="muted small">
              To install it, copy the file to your phone and open it (allow installs from this source if asked), or
              run <code>adb install "{stage.result.path}"</code>.
              {stage.result.package === info?.package && " Uninstall the original game first: it has the same package name."}
            </p>
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
              <pre className="code">{stage.error}</pre>
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
