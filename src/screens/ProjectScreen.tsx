import { useCallback, useEffect, useState } from "react";
import AiPanel from "../components/AiPanel";
import BuildDialog from "../components/BuildDialog";
import CategoryTree from "../components/CategoryTree";
import Inspector from "../components/Inspector";
import { api, containerLabel, formatBytes } from "../lib/api";
import { statusLabel } from "../lib/categories";
import type { Category, EditState, GameProject, ImportRecord } from "../lib/types";

type Tab = "content" | "overview";

interface Props {
  project: ImportRecord;
  /** True while this project is the one on screen. */
  active: boolean;
  /** Focus mode: the app sidebar and this project's tree are hidden. */
  focus: boolean;
  onToggleFocus: () => void;
}

/** What the AI can read in a tab: the file shown and any selected text. */
interface TabContext {
  file: string | null;
  selection: string;
}

export default function ProjectScreen({ project, active: shown, focus, onToggleFocus }: Props) {
  const [analysis, setAnalysis] = useState<GameProject | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<Tab>("content");
  // Open objects, like editor tabs: each keeps its state while in the background.
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [contexts, setContexts] = useState<Record<string, TabContext>>({});
  // Files the AI group has been used with, so each keeps its conversation.
  const [aiFiles, setAiFiles] = useState<string[]>([]);
  const [aiOpen, setAiOpen] = useState(false);
  // Goes up each time the project comes back on screen, so the AI panel
  // picks up a key or model changed in Settings meanwhile.
  const [returns, setReturns] = useState(0);

  useEffect(() => {
    if (shown) setReturns((n) => n + 1);
  }, [shown]);
  const [edits, setEdits] = useState<EditState>({ edited: [], undo: null });
  const [revision, setRevision] = useState(0);
  const [building, setBuilding] = useState(false);

  const onEdited = (state: EditState) => {
    setEdits(state);
    setRevision((r) => r + 1);
  };

  const load = useCallback(
    async (refresh: boolean) => {
      setBusy(true);
      setError(null);
      try {
        setAnalysis(await api.getAnalysis(project.id, refresh));
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [project.id],
  );

  useEffect(() => {
    setAnalysis(null);
    setOpenTabs([]);
    setActiveId(null);
    setContexts({});
    setAiFiles([]);
    load(false);
    api.getEditState(project.id).then(setEdits).catch(console.error);
  }, [load, project.id]);

  const undo = async () => {
    try {
      onEdited(await api.undoEdit(project.id));
    } catch (e) {
      setError(`Couldn't undo: ${e}`);
    }
  };

  const move = async (objectId: string, category: Category | null) => {
    const apply = (to: Category | null) =>
      setAnalysis((prev) =>
        prev && {
          ...prev,
          objects: prev.objects.map((o) =>
            o.id === objectId ? { ...o, category: to ?? o.autoCategory } : o,
          ),
        },
      );
    const before = analysis?.objects.find((o) => o.id === objectId);
    apply(category);
    try {
      await api.setCategory(project.id, objectId, category);
    } catch (e) {
      if (before) apply(before.category === before.autoCategory ? null : before.category);
      setError(`Couldn't save the change: ${e}`);
    }
  };

  const openObject = (id: string) => {
    setOpenTabs((tabs) => (tabs.includes(id) ? tabs : [...tabs, id]));
    setActiveId(id);
  };

  const closeTab = (id: string) => {
    const index = openTabs.indexOf(id);
    const rest = openTabs.filter((t) => t !== id);
    setOpenTabs(rest);
    if (activeId === id) setActiveId(rest[Math.min(index, rest.length - 1)] ?? null);
  };

  const edited = new Set(edits.edited);
  const tabObjects = openTabs
    .map((id) => analysis?.objects.find((o) => o.id === id))
    .filter((o): o is NonNullable<typeof o> => !!o);
  const active = activeId ? contexts[activeId] : undefined;
  const aiFile = active?.file ?? null;

  // The AI group follows the active tab's file.
  useEffect(() => {
    if (aiOpen && aiFile) setAiFiles((files) => (files.includes(aiFile) ? files : [...files, aiFile]));
  }, [aiOpen, aiFile]);
  const importedAt = new Date(project.importedAt * 1000).toLocaleString();

  return (
    <div className="project">
      <header className="project-head">
        <div>
          <h1>{project.name}</h1>
          <p className="muted">
            {containerLabel[project.container]}
            {analysis && <> · {analysis.engine.name}</>} · imported {importedAt}
          </p>
        </div>
        <div className="row">
          <div className="tabs">
            <button className={tab === "content" ? "tab active" : "tab"} onClick={() => setTab("content")}>
              Content
            </button>
            <button className={tab === "overview" ? "tab active" : "tab"} onClick={() => setTab("overview")}>
              Overview
            </button>
          </div>
          <button onClick={() => load(true)} disabled={busy} title="Run the analyzer again. Your manual changes are kept.">
            {busy ? "Analyzing…" : "Re-analyze"}
          </button>
          <button onClick={undo} disabled={!edits.undo} title={edits.undo ? `Undo: ${edits.undo}` : "Nothing to undo"}>
            Undo
          </button>
          {project.container === "apk" && (
            <button className="primary" onClick={() => setBuilding(true)}>
              Build APK…
            </button>
          )}
        </div>
      </header>

      {error && <div className="panel error project-error">{error}</div>}
      {building && (
        <BuildDialog projectId={project.id} editedCount={edits.edited.length} onClose={() => setBuilding(false)} />
      )}

      {analysis ? (
          // Kept mounted under Overview too, so open tabs keep their state.
          <div className="browser" hidden={tab !== "content"}>
            <aside className="tree-pane" hidden={focus}>
              <CategoryTree
                objects={analysis.objects}
                edited={edited}
                selectedId={activeId}
                onSelect={openObject}
              />
            </aside>
            <section className="editor-group">
                <div className="tab-bar">
                  <div className="editor-tools">
                    <button
                      className="tool-button"
                      onClick={onToggleFocus}
                      title={focus ? "Show the side panels" : "Hide the side panels"}
                      aria-pressed={focus}
                    >
                      {focus ? "»" : "«"}
                    </button>
                    <button
                      className={aiOpen ? "tool-button ai-toggle active" : "tool-button ai-toggle"}
                      onClick={() => setAiOpen((o) => !o)}
                      title={aiOpen ? "Close the AI group" : "Open the AI group beside the active file"}
                      aria-pressed={aiOpen}
                    >
                      <span aria-hidden="true">✦</span> AI
                    </button>
                  </div>
                  <div className="tabs-scroll" role="tablist">
                  {tabObjects.map((o) => (
                    <div
                      key={o.id}
                      role="tab"
                      aria-selected={o.id === activeId}
                      className={o.id === activeId ? "editor-tab active" : "editor-tab"}
                      title={`${o.displayName} (${o.kind})`}
                      onClick={() => setActiveId(o.id)}
                      onAuxClick={(e) => e.button === 1 && closeTab(o.id)}
                    >
                      <span className="editor-tab-name">{o.displayName}</span>
                      {o.sourceFiles.some((f) => edited.has(f)) && <span className="edited-dot" title="Edited" />}
                      <button
                        className="editor-tab-close"
                        aria-label={`Close ${o.displayName}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          closeTab(o.id);
                        }}
                      >
                        ×
                      </button>
                    </div>
                  ))}
                  </div>
                </div>
              <div className="inspector-pane">
              {tabObjects.map((o) => (
                <div key={o.id} hidden={o.id !== activeId}>
                  <Inspector
                    projectId={project.id}
                    object={o}
                    edited={edited}
                    revision={revision}
                    onMove={(c) => move(o.id, c)}
                    onEdited={onEdited}
                    onContext={(file, selection) =>
                      setContexts((c) => ({ ...c, [o.id]: { file, selection } }))
                    }
                  />
                </div>
              ))}
              {tabObjects.length === 0 && (
                <div className="empty muted">
                  <p>
                    {analysis.objects.length.toLocaleString()} objects. Pick one on the left to see what it is and
                    preview it.
                  </p>
                  <p className="small">
                    If something is in the wrong category, open it and change its category. Your change is kept
                    when the game is analyzed again.
                  </p>
                  <p className="small">
                    To change the game, open an object and edit or replace its files. Code is changed in each
                    class's smali file. Originals are never touched, and Undo reverts your last change. When
                    you're done, use Build APK.
                  </p>
                  <p className="small">
                    For AI help, click ✦ AI at the top left of this panel: it opens beside the file you have
                    open. « hides the side panels to give it more room.
                  </p>
                </div>
              )}
              </div>
            </section>
            {aiOpen && (
              <aside className="ai-group">
                <div className="ai-group-head">
                  <span>
                    <span aria-hidden="true">✦</span> AI
                  </span>
                  <span className="muted small ai-group-file" title={aiFile ?? undefined}>
                    {aiFile ? `Reading ${aiFile.split("/").pop()}` : "No file open"}
                  </span>
                  <button className="editor-tab-close" aria-label="Close the AI group" onClick={() => setAiOpen(false)}>
                    ×
                  </button>
                </div>
                <div className="ai-group-body">
                  {!aiFile && (
                    <p className="muted small">Open a file from the tree on the left. The AI reads the active tab.</p>
                  )}
                  {aiFiles.map((f) => (
                    <div key={f} hidden={f !== aiFile}>
                      <AiPanel
                        projectId={project.id}
                        path={f}
                        selection={f === aiFile ? (active?.selection ?? "") : ""}
                        refreshKey={returns}
                        onEdited={onEdited}
                      />
                    </div>
                  ))}
                </div>
              </aside>
            )}
          </div>
        ) : (
          !error && tab === "content" && <div className="screen muted">Analyzing the game…</div>
        )}

      {tab === "overview" && (
        <div className="screen">
          {analysis && (
            <>
              <section className="panel">
                <h2>Engine</h2>
                <p>
                  <strong>{analysis.engine.name}</strong>{" "}
                  <span className="muted small">
                    {analysis.engine.confidence >= 0.9
                      ? "detected with high confidence"
                      : analysis.engine.confidence >= 0.5
                        ? "detected with medium confidence"
                        : "no engine recognized"}
                  </span>
                </p>
                <ul className="evidence">
                  {analysis.engine.evidence.map((e) => (
                    <li key={e}>{e}</li>
                  ))}
                  {analysis.notes.map((n) => (
                    <li key={n} className="muted">
                      {n}
                    </li>
                  ))}
                </ul>
              </section>

              <section className="panel">
                <h2>What GameX can do with this game</h2>
                <ul className="capabilities">
                  {analysis.capabilities.map((c) => (
                    <li key={c.feature}>
                      <span className={`status ${c.status}`}>{statusLabel[c.status]}</span>
                      <div>
                        <div>{c.label}</div>
                        <div className="muted small">{c.note}</div>
                      </div>
                    </li>
                  ))}
                </ul>
              </section>
            </>
          )}

          <section className="panel">
            <h2>Unpacking</h2>
            <ul className="steps">
              {project.steps.map((step) => (
                <li key={step.id} className={`step ${step.status}`}>
                  <span className="step-status">{step.status}</span>
                  <div>
                    <div>{step.label}</div>
                    {step.output && <div className="muted small mono">{step.output}</div>}
                    {step.note && <div className="muted small">{step.note}</div>}
                  </div>
                </li>
              ))}
            </ul>
          </section>

          <section className="panel">
            <h2>Details</h2>
            <dl className="details">
              <dt>Source</dt>
              <dd className="mono">{project.sourcePath}</dd>
              <dt>Original size</dt>
              <dd>{formatBytes(project.originalBytes)}</dd>
              {project.originalSha256 && (
                <>
                  <dt>SHA-256</dt>
                  <dd className="mono">{project.originalSha256}</dd>
                </>
              )}
              <dt>Unpacked files</dt>
              <dd>{project.unpackedFiles.toLocaleString()}</dd>
              <dt>Workspace</dt>
              <dd className="mono">{project.workspacePath}</dd>
            </dl>
          </section>
        </div>
      )}
    </div>
  );
}
