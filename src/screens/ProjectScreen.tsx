import {
  FolderOutput,
  Play,
  Gamepad2,
  Hammer,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  RefreshCw,
  Search,
  Settings,
  Sparkles,
  Sun,
  Undo2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import AiPanel from "../components/AiPanel";
import BuildDialog from "../components/BuildDialog";
import RunDialog from "../components/RunDialog";
import CategoryTree from "../components/CategoryTree";
import LiveView from "../components/LiveView";
import Inspector from "../components/Inspector";
import { api, formatBytes, pickPath } from "../lib/api";
import { statusLabel } from "../lib/categories";
import { currentTheme, setTheme, type Theme } from "../lib/theme";
import type { AiStatus, Category, EditState, GameObject, GameProject, ImportRecord, ModelInfo } from "../lib/types";
import { ObjectIcon, containerLongLabel } from "../lib/visuals";

type Tab = "content" | "live" | "overview";

interface Props {
  project: ImportRecord;
  /** True while this project is the one on screen. */
  active: boolean;
  /** Focus mode: the app sidebar and this project's tree are hidden. */
  focus: boolean;
  onToggleFocus: () => void;
  onOpenSettings: () => void;
}

/** What the AI can read in a tab: the file shown and any selected text. */
interface TabContext {
  file: string | null;
  selection: string;
}

/** The first word of a name: "player_walk.png" -> "player", "PlayerHud" -> "player". */
function nameRoot(name: string): string | null {
  const word = name.replace(/\.[^.]+$/, "").match(/[A-Za-z][a-z]*|[A-Z]+(?![a-z])/);
  const root = word?.[0].toLowerCase() ?? null;
  return root && root.length >= 3 ? root : null;
}

export default function ProjectScreen({ project, active: shown, focus, onToggleFocus, onOpenSettings }: Props) {
  const [analysis, setAnalysis] = useState<GameProject | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tab, setTab] = useState<Tab>("content");
  // Open objects, like editor tabs: each keeps its state while in the background.
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [contexts, setContexts] = useState<Record<string, TabContext>>({});
  const [edits, setEdits] = useState<EditState>({ edited: [], undo: null });
  const [revision, setRevision] = useState(0);
  const [building, setBuilding] = useState(false);
  // The Run window stays mounted once opened, so it keeps watching the game.
  const [runMounted, setRunMounted] = useState(false);
  const [runOpen, setRunOpen] = useState(false);
  const [gameRunning, setGameRunning] = useState(false);
  const [icon, setIcon] = useState<string | null>(null);
  const [theme, setThemeState] = useState<Theme>(currentTheme);
  const searchRef = useRef<HTMLInputElement>(null);

  // AI group
  const [aiOpen, setAiOpen] = useState(true);
  // Files the AI group has been used with, so each keeps its conversation.
  const [aiFiles, setAiFiles] = useState<string[]>([]);
  const [aiStatus, setAiStatus] = useState<AiStatus | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);

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
    if (project.container === "apk") {
      api
        .getBuildInfo(project.id)
        .then((info) => setIcon(info.icon))
        .catch(() => setIcon(null));
    }
  }, [load, project.id, project.container]);

  // Pick up a key or model changed in Settings whenever the project comes back on screen.
  useEffect(() => {
    if (!shown) return;
    api
      .aiStatus()
      .then((status) => {
        setAiStatus(status);
        if (status.provider) api.aiModels().then(setModels).catch(() => setModels([]));
      })
      .catch(() => setAiStatus(null));
  }, [shown]);

  const chooseModel = async (model: string) => {
    try {
      setAiStatus(await api.aiSetModel(model));
    } catch (e) {
      setError(String(e));
    }
  };

  const undo = async () => {
    try {
      onEdited(await api.undoEdit(project.id));
    } catch (e) {
      setError(`Couldn't undo: ${e}`);
    }
  };

  // ZIP and folder imports: copy the files out with the edits applied.
  const exportFolder = async () => {
    const dest = await pickPath("folder");
    if (!dest) return;
    try {
      const edited = await api.exportFiles(project.id, dest);
      window.alert(`Exported to ${dest} with ${edited} edited file${edited === 1 ? "" : "s"}.`);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggleTheme = () => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    setThemeState(next);
  };

  const focusSearch = () => {
    setTab("content");
    if (focus) onToggleFocus();
    requestAnimationFrame(() => searchRef.current?.focus());
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

  const byId = useMemo(() => new Map((analysis?.objects ?? []).map((o) => [o.id, o])), [analysis]);
  // Objects grouped by the first word of their name, for "Related Objects".
  const byRoot = useMemo(() => {
    const map = new Map<string, GameObject[]>();
    for (const o of analysis?.objects ?? []) {
      const root = nameRoot(o.displayName);
      if (root) map.set(root, [...(map.get(root) ?? []), o]);
    }
    return map;
  }, [analysis]);
  const relatedTo = (o: GameObject) => {
    const root = nameRoot(o.displayName);
    return (root ? (byRoot.get(root) ?? []) : []).filter((r) => r.id !== o.id).slice(0, 8);
  };

  const openObject = (id: string) => {
    if (!byId.has(id)) return;
    setOpenTabs((tabs) => (tabs.includes(id) ? tabs : [...tabs, id]));
    setActiveId(id);
    setTab("content");
  };

  const closeTab = (id: string) => {
    const index = openTabs.indexOf(id);
    const rest = openTabs.filter((t) => t !== id);
    setOpenTabs(rest);
    if (activeId === id) setActiveId(rest[Math.min(index, rest.length - 1)] ?? null);
  };

  const edited = new Set(edits.edited);
  const tabObjects = openTabs.map((id) => byId.get(id)).filter((o): o is GameObject => !!o);
  const activeContext = activeId ? contexts[activeId] : undefined;
  const aiFile = activeContext?.file ?? null;

  // The AI group follows the active tab's file.
  useEffect(() => {
    if (aiOpen && aiFile) setAiFiles((files) => (files.includes(aiFile) ? files : [...files, aiFile]));
  }, [aiOpen, aiFile]);

  const importedAt = new Date(project.importedAt * 1000).toLocaleString();
  const analyzedAt = analysis ? new Date(analysis.analyzedAt * 1000).toLocaleString() : null;

  return (
    <div className="project">
      <header className="project-head">
        <div className="project-identity">
          <div className="project-avatar">{icon ? <img src={icon} alt="" /> : <Gamepad2 size={26} />}</div>
          <div>
            <div className="project-title">
              <h1>{project.name}</h1>
              {busy ? (
                <span className="status-badge working">Analyzing…</span>
              ) : analysis ? (
                <span className="status-badge ok">Analyzed</span>
              ) : null}
            </div>
            <p className="muted project-subtitle">
              {containerLongLabel[project.container]}
              {analysis && <span className="sep">•</span>}
              {analysis?.engine.name}
              <span className="sep">•</span>
              Imported {importedAt}
            </p>
          </div>
        </div>
        <div className="head-actions">
          <div className="segmented">
            <button className={tab === "content" ? "active" : ""} onClick={() => setTab("content")}>
              Content
            </button>
            {project.container === "apk" && (
              <button className={tab === "live" ? "active" : ""} onClick={() => setTab("live")}>
                Live
              </button>
            )}
            <button className={tab === "overview" ? "active" : ""} onClick={() => setTab("overview")}>
              Overview
            </button>
          </div>
          <button className="icon-button" onClick={focusSearch} title="Search objects">
            <Search size={18} />
          </button>
          <button
            className="icon-button"
            onClick={undo}
            disabled={!edits.undo}
            title={edits.undo ? `Undo: ${edits.undo}` : "Nothing to undo"}
          >
            <Undo2 size={18} />
          </button>
          <button
            className="icon-button"
            onClick={() => load(true)}
            disabled={busy}
            title="Analyze again (your manual changes are kept)"
          >
            <RefreshCw size={18} className={busy ? "spin" : undefined} />
          </button>
          <button className="icon-button" onClick={toggleTheme} title={theme === "dark" ? "Light theme" : "Dark theme"}>
            {theme === "dark" ? <Sun size={18} /> : <Moon size={18} />}
          </button>
          {project.container === "apk" && (
            <button
              className="build-button run-button"
              onClick={() => {
                setRunMounted(true);
                setRunOpen(true);
              }}
              title="Run the game on an emulator or phone"
            >
              <Play size={16} />
              Run
              {gameRunning && <span className="status-dot ok" title="The game is running" />}
            </button>
          )}
          {project.container === "apk" ? (
            <button className="primary build-button" onClick={() => setBuilding(true)}>
              <Hammer size={16} />
              Build APK
            </button>
          ) : (
            <button
              className="primary build-button"
              onClick={exportFolder}
              title="Copy the game's files, with your edits, into an empty folder"
            >
              <FolderOutput size={16} />
              Export
            </button>
          )}
        </div>
      </header>

      {error && <div className="panel error project-error">{error}</div>}
      {runMounted && (
        <RunDialog
          projectId={project.id}
          open={runOpen}
          onClose={() => setRunOpen(false)}
          onRunning={setGameRunning}
          onOpenSettings={onOpenSettings}
          onEditLive={() => {
            setRunOpen(false);
            setTab("live");
          }}
        />
      )}
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
              searchRef={searchRef}
            />
          </aside>

          <section className="editor-group">
            <div className="tab-bar">
              <button
                className="tab-tool"
                onClick={onToggleFocus}
                title={focus ? "Show the side panels" : "Hide the side panels"}
                aria-pressed={focus}
              >
                {focus ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
              </button>
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
                    <ObjectIcon object={o} />
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
                      <X size={13} />
                    </button>
                  </div>
                ))}
              </div>
              <button className="tab-tool" onClick={focusSearch} title="Open another object">
                <Plus size={16} />
              </button>
              {!aiOpen && (
                <button className="tab-tool ai-reopen" onClick={() => setAiOpen(true)} title="Open the AI Assistant">
                  <Sparkles size={15} />
                  <span>AI</span>
                </button>
              )}
            </div>

            <div className="inspector-pane">
              {tabObjects.map((o) => (
                <div key={o.id} hidden={o.id !== activeId}>
                  <Inspector
                    projectId={project.id}
                    object={o}
                    edited={edited}
                    revision={revision}
                    related={relatedTo(o)}
                    onOpen={openObject}
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
                    {analysis.objects.length.toLocaleString()} objects. Pick one on the left to open it in a tab.
                  </p>
                  <p className="small">
                    Wrong category? Open the object and change it under Details; your change is kept when the game
                    is analyzed again.
                  </p>
                  <p className="small">
                    To change the game, edit or replace files under Code (code is changed in each class's smali).
                    Originals are never touched, Undo reverts your last change, and Build APK makes the new game.
                  </p>
                </div>
              )}
            </div>

            <footer className="status-bar">
              <span className={busy ? "status-dot working" : error ? "status-dot bad" : "status-dot ok"} />
              <span>{busy ? "Analyzing…" : error ? "Something went wrong" : "Analysis complete"}</span>
              {edits.edited.length > 0 && (
                <span className="muted">
                  · {edits.edited.length} edited file{edits.edited.length === 1 ? "" : "s"}
                </span>
              )}
              <span className="status-spacer" />
              {analyzedAt && <span className="muted">Last updated: {analyzedAt}</span>}
            </footer>
          </section>

          {aiOpen && (
            <aside className="ai-group">
              <div className="ai-group-head">
                <Sparkles size={18} className="ai-mark" />
                <span className="ai-group-title">AI Assistant</span>
                {aiStatus?.provider && (
                  <select
                    className="model-select"
                    value={aiStatus.model ?? ""}
                    onChange={(e) => chooseModel(e.target.value)}
                    title={aiStatus.providerLabel ?? undefined}
                  >
                    {!aiStatus.model && <option value="">Choose a model</option>}
                    {aiStatus.model && !models.some((m) => m.id === aiStatus.model) && (
                      <option value={aiStatus.model}>{aiStatus.model}</option>
                    )}
                    {models.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.name}
                      </option>
                    ))}
                  </select>
                )}
                <button className="icon-button small-icon" onClick={onOpenSettings} title="AI settings">
                  <Settings size={16} />
                </button>
                <button className="icon-button small-icon" onClick={() => setAiOpen(false)} title="Close">
                  <X size={16} />
                </button>
              </div>
              <div className="ai-group-body">
                {!aiFile && aiStatus?.configured && (
                  <p className="muted small">Open an object from the tree. The assistant reads the active tab.</p>
                )}
                {(!aiStatus?.configured || !aiFile) && (
                  <AiPanel
                    projectId={project.id}
                    path={null}
                    selection=""
                    status={aiStatus}
                    onEdited={onEdited}
                    onOpenSettings={onOpenSettings}
                  />
                )}
                {aiStatus?.configured &&
                  aiFiles.map((f) => (
                    <div key={f} hidden={f !== aiFile}>
                      <AiPanel
                        projectId={project.id}
                        path={f}
                        selection={f === aiFile ? (activeContext?.selection ?? "") : ""}
                        status={aiStatus}
                        onEdited={onEdited}
                        onOpenSettings={onOpenSettings}
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

      {analysis && project.container === "apk" && (
        // Kept mounted so the connection and selection survive tab switches.
        <div className="live-host" hidden={tab !== "live"}>
          <LiveView
            projectId={project.id}
            objects={analysis.objects}
            onOpenObject={openObject}
            onEdited={onEdited}
            onBuild={() => setBuilding(true)}
            visible={shown && tab === "live"}
            onRun={() => {
              setRunMounted(true);
              setRunOpen(true);
            }}
          />
        </div>
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
