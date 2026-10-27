import { useCallback, useEffect, useState } from "react";
import BuildDialog from "../components/BuildDialog";
import CategoryTree from "../components/CategoryTree";
import Inspector from "../components/Inspector";
import { api, containerLabel, formatBytes } from "../lib/api";
import { statusLabel } from "../lib/categories";
import type { Category, EditState, GameProject, ImportRecord } from "../lib/types";

type Tab = "content" | "overview";

export default function ProjectScreen({ project }: { project: ImportRecord }) {
  const [analysis, setAnalysis] = useState<GameProject | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<Tab>("content");
  const [selectedId, setSelectedId] = useState<string | null>(null);
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
    setSelectedId(null);
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

  const selected = analysis?.objects.find((o) => o.id === selectedId) ?? null;
  const edited = new Set(edits.edited);
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

      {tab === "content" &&
        (analysis ? (
          <div className="browser">
            <aside className="tree-pane">
              <CategoryTree
                objects={analysis.objects}
                edited={edited}
                selectedId={selectedId}
                onSelect={setSelectedId}
              />
            </aside>
            <section className="inspector-pane">
              {selected ? (
                <Inspector
                  key={selected.id}
                  projectId={project.id}
                  object={selected}
                  edited={edited}
                  revision={revision}
                  onMove={(c) => move(selected.id, c)}
                  onEdited={onEdited}
                />
              ) : (
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
                </div>
              )}
            </section>
          </div>
        ) : (
          !error && <div className="screen muted">Analyzing the game…</div>
        ))}

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
