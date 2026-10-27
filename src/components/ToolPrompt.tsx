import { Download, Wrench } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { ToolInfo, ToolProgress } from "../lib/types";
import { ProgressBar, installTool } from "./ToolsPanel";

interface Props {
  /** Tool ids that are missing. */
  missing: string[];
  /** What the user was trying to do, e.g. "Importing this APK". */
  action: string;
  /** What happens without them, e.g. "the APK is only extracted, not decoded". */
  without?: string;
  /** Called once every missing tool is installed. */
  onReady: () => void;
  /** "Continue anyway" (omit to offer only Cancel). */
  onSkip?: () => void;
  onCancel: () => void;
}

/** Asks to install the tools an action needs, with progress, then continues. */
export default function ToolPrompt({ missing, action, without, onReady, onSkip, onCancel }: Props) {
  const [tools, setTools] = useState<ToolInfo[]>([]);
  const [done, setDone] = useState<Set<string>>(new Set());
  const [current, setCurrent] = useState<string | null>(null);
  const [progress, setProgress] = useState<ToolProgress | undefined>();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.toolsCatalog().then((all) => setTools(all.filter((t) => missing.includes(t.id))));
  }, [missing]);

  const installAll = async () => {
    setError(null);
    for (const tool of tools) {
      if (done.has(tool.id)) continue;
      setCurrent(tool.id);
      setProgress(undefined);
      try {
        await installTool(tool.id, setProgress);
        setDone((d) => new Set(d).add(tool.id));
      } catch (e) {
        setError(`${tool.name}: ${e}`);
        setCurrent(null);
        return;
      }
    }
    setCurrent(null);
    onReady();
  };

  const working = current !== null;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !working && onCancel()}>
      <div className="modal tool-prompt" role="dialog" aria-modal="true">
        <h2>
          <Wrench size={18} /> Tools needed
        </h2>
        <p>
          {action} needs {tools.length === 1 ? "a tool that isn't" : "tools that aren't"} installed yet.
          {without && <span className="muted"> Without {tools.length === 1 ? "it" : "them"}, {without}.</span>}
        </p>
        <ul className="tool-list">
          {tools.map((t) => (
            <li key={t.id} className="tool-card">
              <div className="tool-main">
                <strong>{t.name}</strong>
                <div className="muted small">
                  {t.description} ({t.size})
                </div>
                {current === t.id && <ProgressBar progress={progress} />}
                {done.has(t.id) && <span className="pill ok">installed</span>}
              </div>
            </li>
          ))}
        </ul>
        {error && <div className="panel error small">{error}</div>}
        <p className="muted tiny">
          Downloaded from each tool's official release into GameX's tools folder. You can manage them later under Settings
          → Tools.
        </p>
        <div className="modal-actions">
          <button onClick={onCancel} disabled={working}>
            Cancel
          </button>
          {onSkip && (
            <button onClick={onSkip} disabled={working}>
              Continue without
            </button>
          )}
          <button className="primary" onClick={installAll} disabled={working || tools.length === 0}>
            <Download size={14} />
            {working ? "Installing…" : `Install${tools.length > 1 ? " all" : ""} and continue`}
          </button>
        </div>
      </div>
    </div>
  );
}
