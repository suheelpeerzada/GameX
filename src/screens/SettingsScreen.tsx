import { useEffect, useState } from "react";
import AiSettings from "../components/AiSettings";
import { api, inDesktopApp, pickPath } from "../lib/api";
import type { Settings, ToolStatus } from "../lib/types";

const TOOLS: { id: "apktool" | "jadx" | "uber-apk-signer"; key: keyof Settings; label: string; help: string }[] = [
  {
    id: "apktool",
    key: "apktoolPath",
    label: "apktool",
    help: "Decodes APK resources and smali. On Windows, point this at apktool.bat.",
  },
  {
    id: "jadx",
    key: "jadxPath",
    label: "jadx",
    help: "Decompiles APK code to readable Java. On Windows, point this at jadx.bat in jadx's bin folder.",
  },
  {
    id: "uber-apk-signer",
    key: "signerPath",
    label: "uber-apk-signer",
    help: "Aligns and signs rebuilt APKs (runs with Java). Point this at uber-apk-signer.jar, or put its folder on PATH.",
  },
];

export default function SettingsScreen() {
  const [settings, setSettings] = useState<Settings>({
    apktoolPath: null,
    jadxPath: null,
    signerPath: null,
  });
  const [status, setStatus] = useState<ToolStatus[]>([]);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!inDesktopApp) return;
    api.getSettings().then(setSettings);
    api.toolStatus().then(setStatus);
  }, []);

  const update = (key: keyof Settings, value: string) => {
    setSaved(false);
    setSettings((s) => ({ ...s, [key]: value.trim() === "" ? null : value }));
  };

  const save = async () => {
    await api.saveSettings(settings);
    setStatus(await api.toolStatus());
    setSaved(true);
  };

  return (
    <div className="screen">
      <h1>Settings</h1>
      <p className="muted">
        GameX runs these tools as separate programs. Leave a path empty to look for the tool on
        PATH. Imports work without apktool and jadx; the APK is then only extracted, not decompiled.
        Building a modified APK needs apktool, uber-apk-signer and Java.
      </p>

      {TOOLS.map((tool) => {
        const s = status.find((t) => t.id === tool.id);
        return (
          <section key={tool.id} className="panel">
            <h2>
              {tool.label}{" "}
              {s && <span className={s.found ? "pill ok" : "pill warn"}>{s.found ? "found" : "missing"}</span>}
            </h2>
            <p className="muted small">{tool.help}</p>
            <div className="row">
              <input
                type="text"
                placeholder="Use PATH"
                value={settings[tool.key] ?? ""}
                onChange={(e) => update(tool.key, e.target.value)}
              />
              <button
                disabled={!inDesktopApp}
                onClick={async () => {
                  const path = await pickPath("tool");
                  if (path) update(tool.key, path);
                }}
              >
                Browse
              </button>
            </div>
            {s && (
              <p className="small muted">
                {s.found ? `Using ${s.path} (from ${s.source})` : s.note}
              </p>
            )}
          </section>
        );
      })}

      <div className="row">
        <button className="primary" disabled={!inDesktopApp} onClick={save}>
          Save
        </button>
        {saved && <span className="muted small">Saved.</span>}
      </div>

      <div className="settings-gap" />
      <AiSettings />
    </div>
  );
}
