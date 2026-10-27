import { openUrl } from "@tauri-apps/plugin-opener";
import { CheckCircle2, Download, ExternalLink, RefreshCw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api, formatBytes, inDesktopApp } from "../lib/api";
import type { ToolInfo, ToolProgress } from "../lib/types";

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
        Optional programs GameX can use. Install only what you need: GameX downloads them from their official releases
        into its own folder, and asks when something you do needs one that's missing. Tools with their own installer are
        found automatically once installed.
      </p>
      <ul className="tool-list">
        {tools.map((tool) => (
          <li key={tool.id} className="tool-card">
            <div className="tool-main">
              <div className="tool-title">
                <strong>{tool.name}</strong>
                {tool.installed ? (
                  <span className="pill ok">
                    <CheckCircle2 size={11} /> {tool.origin === "detected" ? "found" : `installed ${tool.version ?? ""}`}
                  </span>
                ) : (
                  <span className="pill muted-pill">not installed</span>
                )}
              </div>
              <div className="muted small">{tool.description}</div>
              <div className="small tool-meta">
                Used for: {tool.usedFor} · {tool.size}
              </div>
              {tool.installed && tool.path && <div className="mono tiny muted">{tool.path}</div>}
              {busy[tool.id] && tool.managed && <ProgressBar progress={progress[tool.id]} />}
              {errors[tool.id] && <div className="error-text small">{errors[tool.id]}</div>}
            </div>
            <div className="tool-actions">
              {tool.managed ? (
                <>
                  <button
                    className={tool.installed ? "" : "primary"}
                    disabled={!inDesktopApp || busy[tool.id]}
                    onClick={() => install(tool)}
                  >
                    {tool.installed ? <RefreshCw size={14} /> : <Download size={14} />}
                    {busy[tool.id] ? "Working…" : tool.installed ? "Update" : "Install"}
                  </button>
                  {tool.installed && tool.origin === "GameX" && (
                    <button disabled={busy[tool.id]} onClick={() => remove(tool)} title="Remove">
                      <Trash2 size={14} />
                    </button>
                  )}
                </>
              ) : (
                <>
                  {!tool.installed && (
                    <button className="primary" onClick={() => openUrl(tool.homepage).catch(() => {})}>
                      <ExternalLink size={14} />
                      Get it
                    </button>
                  )}
                  <button onClick={refresh} title="Look for it again">
                    <RefreshCw size={14} />
                    Check again
                  </button>
                </>
              )}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
