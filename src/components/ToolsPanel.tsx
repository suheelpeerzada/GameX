import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckCircle2, ChevronDown, ChevronUp, Download, ExternalLink, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, formatBytes, inDesktopApp, pickPath } from "../lib/api";
import type { Settings, ToolInfo, ToolProgress } from "../lib/types";

/** Tools whose path can be set by hand, and the setting that holds it. */
const PATH_KEYS: Record<string, keyof Settings> = {
  apktool: "apktoolPath",
  jadx: "jadxPath",
  "uber-apk-signer": "signerPath",
};

/** Downloads one tool, reporting progress. Shared by the panel and prompts. */
export async function installTool(id: string, onProgress: (p: ToolProgress) => void): Promise<ToolInfo> {
  const unlisten = await api.onToolProgress((p) => p.id === id && onProgress(p));
  try {
    return await api.toolInstall(id);
  } finally {
    unlisten();
  }
}

export function ProgressBar({ progress }: { progress: ToolProgress | undefined }) {
  if (!progress) return <span className="muted small">Starting the download…</span>;
  const pct = progress.total ? Math.min(100, Math.round((progress.received / progress.total) * 100)) : null;
  return (
    <span className="tool-progress">
      <span className="bar">
        <span style={{ width: `${pct ?? 30}%` }} />
      </span>
      <span className="muted small">
        {formatBytes(progress.received)}
        {progress.total ? ` of ${formatBytes(progress.total)}` : ""}
      </span>
    </span>
  );
}

/** Settings → Tools: optional tools to install, update or remove. */
export default function ToolsPanel({ onChanged }: { onChanged?: () => void }) {
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [progress, setProgress] = useState<Record<string, ToolProgress>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [settings, setSettings] = useState<Settings | null>(null);
  // Custom paths being typed, not saved yet.
  const [drafts, setDrafts] = useState<Record<string, string>>({});

  useEffect(() => {
    if (inDesktopApp) api.getSettings().then(setSettings).catch(console.error);
  }, []);

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const savePath = async (id: string, value: string) => {
    const key = PATH_KEYS[id];
    if (!key || !settings) return;
    const next = { ...settings, [key]: value.trim() === "" ? null : value.trim() };
    try {
      await api.saveSettings(next);
      setSettings(next);
      setDrafts(({ [id]: _, ...rest }) => rest);
      setErrors((x) => ({ ...x, [id]: "" }));
    } catch (e) {
      setErrors((x) => ({ ...x, [id]: String(e) }));
    }
    refresh();
    onChanged?.();
  };

  const refresh = useCallback(() => {
    if (inDesktopApp) api.toolsCatalog().then(setTools).catch(console.error);
  }, []);
  useEffect(refresh, [refresh]);

  const act = async (tool: ToolInfo, action: () => Promise<unknown>) => {
    setBusy((b) => ({ ...b, [tool.id]: true }));
    setErrors((e) => ({ ...e, [tool.id]: "" }));
    try {
      await action();
    } catch (e) {
      setErrors((x) => ({ ...x, [tool.id]: String(e) }));
    } finally {
      setBusy((b) => ({ ...b, [tool.id]: false }));
      setProgress((p) => {
        const { [tool.id]: _, ...rest } = p;
        return rest;
      });
      refresh();
      onChanged?.();
    }
  };

  const install = (tool: ToolInfo) =>
    act(tool, () => installTool(tool.id, (p) => setProgress((x) => ({ ...x, [tool.id]: p }))));
  const remove = (tool: ToolInfo) => {
    if (!window.confirm(`Remove ${tool.name} from GameX?`)) return;
    act(tool, () => api.toolUninstall(tool.id));
  };

  return (
    <section className="panel">
      <h2>Tools</h2>
      <p className="muted small">
        Optional programs GameX can use. Install only what you need; GameX asks when something you do needs one that's
        missing. Expand a tool for details and a custom path.
      </p>
      <ul className="tool-list">
        {tools.map((tool) => {
          const open = expanded.has(tool.id);
          const pathKey = PATH_KEYS[tool.id];
          return (
            <li key={tool.id} className="tool-card">
              <div
                className="tool-row"
                role="button"
                tabIndex={0}
                aria-expanded={open}
                onClick={() => toggle(tool.id)}
                onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), toggle(tool.id))}
              >
                <strong>{tool.name}</strong>
                <div className="tool-status">
                  {tool.installed ? (
                    <span className="pill ok">
                      <CheckCircle2 size={11} />{" "}
                      {tool.origin === "GameX"
                        ? `installed ${tool.version ?? ""}`
                        : tool.origin === "settings"
                          ? "your custom path"
                          : "found on your PC"}
                    </span>
                  ) : (
                    <span className="pill muted-pill">not installed</span>
                  )}
                  <span className="tool-expand" title={open ? "Hide details" : "Show details"}>
                    {open ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
                  </span>
                </div>
              </div>
              {busy[tool.id] && tool.managed && <ProgressBar progress={progress[tool.id]} />}
              {errors[tool.id] && <div className="error-text small">{errors[tool.id]}</div>}

              {open && (
                <div className="tool-details">
                  <div className="muted small">{tool.description}</div>
                  <div className="small tool-meta">
                    Used for: {tool.usedFor} · {tool.size}
                  </div>
                  {tool.installed && tool.path && (
                    <div className="small">
                      In use: <span className="mono tiny muted">{tool.path}</span>
                    </div>
                  )}
                  {pathKey && (
                    <div className="tool-path">
                      <span className="small">Custom path</span>
                      <div className="row">
                        <input
                          type="text"
                          placeholder="Not set: GameX's copy or PATH is used"
                          value={drafts[tool.id] ?? settings?.[pathKey] ?? ""}
                          onChange={(e) => setDrafts((d) => ({ ...d, [tool.id]: e.target.value }))}
                        />
                        <button
                          disabled={!inDesktopApp}
                          onClick={async () => {
                            const picked = await pickPath("tool");
                            if (picked) setDrafts((d) => ({ ...d, [tool.id]: picked }));
                          }}
                        >
                          Browse
                        </button>
                        <button
                          className="primary"
                          disabled={!inDesktopApp || drafts[tool.id] === undefined}
                          onClick={() => savePath(tool.id, drafts[tool.id] ?? "")}
                        >
                          Save
                        </button>
                        {settings?.[pathKey] && (
                          <button onClick={() => savePath(tool.id, "")} title="Stop using a custom path">
                            Clear
                          </button>
                        )}
                      </div>
                      <span className="muted tiny">A custom path always wins over GameX's copy and PATH.</span>
                    </div>
                  )}
                  <div className="row tool-more">
                    {tool.managed && (!tool.installed || tool.origin === "GameX") && (
                      <button
                        className={tool.installed ? "" : "primary"}
                        disabled={!inDesktopApp || busy[tool.id]}
                        onClick={() => install(tool)}
                      >
                        {tool.installed ? <RefreshCw size={14} /> : <Download size={14} />}
                        {busy[tool.id] ? "Working…" : tool.installed ? "Update" : `Install (${tool.size})`}
                      </button>
                    )}
                    {!tool.managed && !tool.installed && (
                      <button className="primary" onClick={() => openUrl(tool.homepage).catch(() => {})}>
                        <ExternalLink size={14} />
                        Get it
                      </button>
                    )}
                    {tool.managed && tool.installed && tool.origin !== "GameX" && (
                      <button disabled={busy[tool.id]} onClick={() => install(tool)}>
                        <Download size={14} />
                        Install GameX's own copy
                      </button>
                    )}
                    {tool.managed && tool.installed && tool.origin === "GameX" && (
                      <button disabled={busy[tool.id]} onClick={() => remove(tool)}>
                        <Trash2 size={14} />
                        Remove
                      </button>
                    )}
                    {!tool.managed && (
                      <button onClick={refresh}>
                        <RefreshCw size={14} />
                        Check again
                      </button>
                    )}
                    <button className="link" onClick={() => openUrl(tool.homepage).catch(() => {})}>
                      Website
                    </button>
                  </div>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
