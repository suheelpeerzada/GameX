import { getVersion } from "@tauri-apps/api/app";
import { Box, ChevronRight, ChevronsLeft, ChevronsRight, FolderOpen, Gamepad2, Home, Info, MonitorPlay, Play, Plus, Radio, Settings, Trash2 } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import logo from "./assets/logo.png";
import { api, inDesktopApp } from "./lib/api";
import type { TranslationReport } from "./lib/gx";
import type { ImportRecord } from "./lib/types";
import { containerLongLabel } from "./lib/visuals";
import ImportScreen from "./screens/ImportScreen";
import ProjectScreen from "./screens/ProjectScreen";

// Loaded when first visited.
const AboutScreen = lazy(() => import("./screens/AboutScreen"));
const SettingsScreen = lazy(() => import("./screens/SettingsScreen"));
const WorkspaceScreen = lazy(() => import("./screens/WorkspaceScreen"));

type View = { kind: "workspace" } | { kind: "import" } | { kind: "settings" } | { kind: "about" } | { kind: "project"; id: string };

/** The top bar's modes for the game on screen. */
type Mode = "files" | "overview" | "editor" | "live" | "play";

/** Same as the backend's project::slug: how a game's ID in a layer is made from its name. */
export function slug(name: string): string {
  const s = name
    .split("")
    .map((c) => (/[a-z0-9]/i.test(c) && c.charCodeAt(0) < 128 ? c.toLowerCase() : "-"))
    .join("")
    .split("-")
    .filter(Boolean)
    .join("-")
    .slice(0, 48);
  return s || "item";
}

const FOLD_KEY = "gamex.gamesFolded";

function readFold(): boolean {
  try {
    return localStorage.getItem(FOLD_KEY) === "1";
  } catch {
    return false;
  }
}

export default function App() {
  const [view, setView] = useState<View>({ kind: "import" });
  const [projects, setProjects] = useState<ImportRecord[]>([]);
  const [identities, setIdentities] = useState<Record<string, { label: string; icon: string | null }>>({});
  const [version, setVersion] = useState<string | null>(null);
  // Projects opened this session stay mounted (hidden when not shown), so
  // tabs, AI answers and unsaved edits survive a trip to Settings.
  const [opened, setOpened] = useState<string[]>([]);
  // Focus mode hides the games panel and the project's tree.
  const [focus, setFocus] = useState(false);
  const [folded, setFolded] = useState(readFold);
  const [mode, setMode] = useState<Mode>("files");
  const [status, setStatus] = useState("");
  // Game Projection asked for before the editor was open: play once it is.
  const [autoplay, setAutoplay] = useState(false);
  // "Export modified APK" in the visual editor opens this game's Build dialog.
  const [buildRequest, setBuildRequest] = useState<{ id: string; n: number }>({ id: "", n: 0 });
  // The top bar asks a game's screen for its editor or Live view.
  const [modeRequest, setModeRequest] = useState<{ id: string; mode: "editor" | "live" | "content" | "overview"; n: number }>({ id: "", mode: "editor", n: 0 });
  // The visual editor, open on one imported game's layer (kept while you visit other screens).
  const [editing, setEditing] = useState<{ game: string; path: string; report?: TranslationReport } | null>(null);
  // The games in the open layer (their IDs there), and requests to it.
  const [layerGames, setLayerGames] = useState<string[]>([]);
  const [addRequest, setAddRequest] = useState<{ id: string; n: number }>({ id: "", n: 0 });
  const [playRequest, setPlayRequest] = useState(0);

  useEffect(() => {
    if (view.kind === "project") {
      setOpened((ids) => (ids.includes(view.id) ? ids : [...ids, view.id]));
    }
  }, [view]);

  const refresh = useCallback(async () => {
    if (!inDesktopApp) return;
    const list = await api.listProjects();
    setProjects(list);
    api.gameIdentities(list.map((p) => p.id)).then(setIdentities).catch(() => {});
  }, []);

  useEffect(() => {
    refresh().catch(console.error);
    if (inDesktopApp) getVersion().then(setVersion).catch(() => {});
  }, [refresh]);

  const toggleFold = () =>
    setFolded((f) => {
      try {
        localStorage.setItem(FOLD_KEY, f ? "0" : "1");
      } catch {
        // Remembering is only a convenience.
      }
      return !f;
    });

  /** A game's name as people know it: the phone's label, else the import's name. */
  const nameOf = useCallback((p: ImportRecord) => identities[p.id]?.label || p.name, [identities]);

  // The game the top bar is about: the one on screen, or the one being edited.
  const current = useMemo(() => {
    const id = view.kind === "project" ? view.id : view.kind === "workspace" ? editing?.game : undefined;
    return projects.find((p) => p.id === id);
  }, [view, editing, projects]);

  const remove = async (project: ImportRecord) => {
    const ok = window.confirm(
      `Delete "${nameOf(project)}" from GameX?\n\nThis removes its workspace, including your edits and history. ` +
        `The file you imported it from is not touched.`,
    );
    if (!ok) return;
    try {
      await api.deleteProject(project.id);
    } catch (e) {
      window.alert(`Couldn't delete the project: ${e}`);
    }
    if (view.kind === "project" && view.id === project.id) setView({ kind: "import" });
    setOpened((ids) => ids.filter((id) => id !== project.id));
    await refresh();
  };

  const openGame = (p: ImportRecord) => {
    setMode("files");
    setView({ kind: "project", id: p.id });
    setStatus(`${nameOf(p)} opened.`);
  };

  const switchMode = (next: Mode) => {
    if (!current) return;
    setMode(next);
    if (next === "files" || next === "overview") {
      setView({ kind: "project", id: current.id });
      setModeRequest((r) => ({ id: current.id, mode: next === "files" ? "content" : "overview", n: r.n + 1 }));
      return;
    }
    if (next === "editor" || next === "play") {
      if (editing?.game === current.id) {
        setView({ kind: "workspace" });
        if (next === "play") setPlayRequest((n) => n + 1);
        return;
      }
      // The game's screen makes or opens its layer, then the editor opens.
      setAutoplay(next === "play");
      setView({ kind: "project", id: current.id });
      setModeRequest((r) => ({ id: current.id, mode: "editor", n: r.n + 1 }));
      return;
    }
    setView({ kind: "project", id: current.id });
    setModeRequest((r) => ({ id: current.id, mode: "live", n: r.n + 1 }));
  };

  /** "Loaded": in the open workspace; with none open, opened this session. */
  const loaded = (p: ImportRecord) => (editing ? editing.game === p.id || layerGames.includes(slug(p.name)) : opened.includes(p.id));

  const modes: { id: Mode; label: string; Icon: typeof Box }[] = [
    { id: "files", label: "Game files", Icon: FolderOpen },
    { id: "overview", label: "Overview", Icon: Info },
    { id: "editor", label: "Editor", Icon: Box },
    { id: "live", label: "Live Preview", Icon: Radio },
    { id: "play", label: "Game Projection", Icon: MonitorPlay },
  ];
  const activeMode: Mode | null = !current ? null : view.kind === "workspace" ? (mode === "play" ? "play" : "editor") : mode === "live" || mode === "overview" ? mode : "files";
  const showPanel = !(focus && view.kind === "project");

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <img src={logo} alt="" className="brand-logo" />
          <span>GameX</span>
        </div>
        <nav className="crumbs">
          <button className={view.kind === "import" ? "crumb-home active" : "crumb-home"} onClick={() => setView({ kind: "import" })} title="Home: import a game">
            <Home size={14} /> Workspace
          </button>
          {current && (
            <>
              <ChevronRight size={14} className="muted" />
              <span className="crumb-game">
                <GameIcon icon={identities[current.id]?.icon} size={22} />
                <select value={current.id} onChange={(e) => projects.find((p) => p.id === e.target.value) && openGame(projects.find((p) => p.id === e.target.value)!)}>
                  {projects.map((p) => (
                    <option key={p.id} value={p.id}>
                      {nameOf(p)}
                    </option>
                  ))}
                </select>
              </span>
            </>
          )}
        </nav>
        {current && (
          <div className="mode-tabs">
            {modes.map(({ id, label, Icon }) => (
              <button key={id} className={activeMode === id ? "active" : ""} onClick={() => switchMode(id)}>
                <Icon size={15} /> {label}
              </button>
            ))}
          </div>
        )}
        <span className="status-spacer" />
        {current && (
          <button className="icon-button" onClick={() => switchMode("play")} title="Play with the GameX engine">
            <Play size={16} />
          </button>
        )}
        <button className={view.kind === "settings" ? "icon-button active" : "icon-button"} onClick={() => setView({ kind: "settings" })} title="Settings">
          <Settings size={16} />
        </button>
        <button className={view.kind === "about" ? "icon-button active" : "icon-button"} onClick={() => setView({ kind: "about" })} title="About GameX">
          <Info size={16} />
        </button>
      </header>

      <div className="shell-body">
        {showPanel && (
          <aside className={folded ? "games-panel folded" : "games-panel"}>
            <div className="games-head">
              {!folded && <strong>Games</strong>}
              <button className={folded ? "icon-button" : "small import-game"} onClick={() => setView({ kind: "import" })} title="Import a game">
                <Plus size={15} /> {!folded && "Import Game"}
              </button>
              <button className="icon-button" onClick={toggleFold} title={folded ? "Show the games" : "Fold the games away"}>
                {folded ? <ChevronsRight size={15} /> : <ChevronsLeft size={15} />}
              </button>
            </div>
            <ul className="game-cards">
              {projects.length === 0 && !folded && <li className="muted small">Nothing imported yet.</li>}
              {projects.map((p) => {
                const active = current?.id === p.id;
                const isLoaded = loaded(p);
                const canAdd = !!editing && view.kind === "workspace" && !isLoaded;
                return (
                  <li key={p.id} className="game-card-item">
                    <button className={active ? "game-card active" : "game-card"} onClick={() => openGame(p)} title={folded ? nameOf(p) : `${nameOf(p)} · ${containerLongLabel[p.container]}`}>
                      <GameIcon icon={identities[p.id]?.icon} size={folded ? 32 : 40} />
                      {!folded && (
                        <span className="game-card-text">
                          <span className="game-name">{nameOf(p)}</span>
                          <span className={isLoaded ? "game-state on" : "game-state"}>{isLoaded ? "Loaded" : "Not Loaded"}</span>
                        </span>
                      )}
                    </button>
                    {!folded && (
                      <span className="game-card-actions">
                        {canAdd && (
                          <button title={`Add ${nameOf(p)} to this workspace`} onClick={() => setAddRequest((r) => ({ id: p.id, n: r.n + 1 }))}>
                            <Plus size={13} />
                          </button>
                        )}
                        <button className="danger" title={`Delete ${nameOf(p)} from GameX`} aria-label={`Delete ${nameOf(p)}`} onClick={() => remove(p)}>
                          <Trash2 size={13} />
                        </button>
                      </span>
                    )}
                  </li>
                );
              })}
            </ul>
          </aside>
        )}
        <main className="content">
          {!inDesktopApp && (
            <div className="banner">
              You are viewing the UI in a browser. Importing needs the desktop app: run <code>npm run tauri dev</code>.
            </div>
          )}
          {view.kind === "import" && (
            <ImportScreen
              onImported={async (record) => {
                await refresh();
                setMode("files");
                setView({ kind: "project", id: record.id });
                setStatus(`${record.name} imported.`);
              }}
            />
          )}
          <Suspense fallback={null}>
            {view.kind === "settings" && <SettingsScreen />}
            {view.kind === "about" && <AboutScreen />}
            {editing && (
              <div className="workspace-host" hidden={view.kind !== "workspace"}>
                <WorkspaceScreen
                  key={editing.path}
                  path={editing.path}
                  report={editing.report}
                  visible={view.kind === "workspace"}
                  game={editing.game}
                  games={projects.map((p) => ({ id: p.id, name: nameOf(p) }))}
                  addRequest={addRequest}
                  playRequest={playRequest}
                  autoplay={autoplay}
                  onAutoplayed={() => setAutoplay(false)}
                  onPlaying={(playing) => setMode(playing ? "play" : "editor")}
                  onPlayReal={() => {
                    // A level built in code: its gameplay is the real game's, in Live Preview.
                    setMode("live");
                    setView({ kind: "project", id: editing.game });
                    setModeRequest((r) => ({ id: editing.game, mode: "live", n: r.n + 1 }));
                    setStatus("This game's levels are built in its code: playing the real game in Live Preview.");
                  }}
                  onSources={setLayerGames}
                  onStatus={setStatus}
                  onClose={() => {
                    setMode("files");
                    setView({ kind: "project", id: editing.game });
                  }}
                  onBuild={() => {
                    setMode("files");
                    setView({ kind: "project", id: editing.game });
                    setBuildRequest((r) => ({ id: editing.game, n: r.n + 1 }));
                  }}
                />
              </div>
            )}
          </Suspense>
          {projects
            .filter((p) => opened.includes(p.id))
            .map((p) => {
              const shown = view.kind === "project" && view.id === p.id;
              return (
                <div key={p.id} className="project-host" hidden={!shown}>
                  <ProjectScreen
                    project={p}
                    label={identities[p.id]?.label}
                    active={shown}
                    focus={focus}
                    onToggleFocus={() => setFocus((f) => !f)}
                    onOpenSettings={() => setView({ kind: "settings" })}
                    buildRequest={buildRequest.id === p.id ? buildRequest.n : 0}
                    modeRequest={modeRequest.id === p.id ? modeRequest : undefined}
                    onTabChange={(t) => {
                      if (view.kind === "project" && view.id === p.id) setMode(t === "content" ? "files" : t);
                    }}
                    onEditVisually={(path, report) => {
                      setEditing({ game: p.id, path, report });
                      setMode("editor");
                      setView({ kind: "workspace" });
                      setStatus(`${nameOf(p)} loaded successfully.`);
                    }}
                  />
                </div>
              );
            })}
        </main>
      </div>

      <footer className="statusbar">
        <span className="status-dot" /> Ready
        <span className="sep">|</span>
        <span className="muted grow">{status}</span>
        <span className="muted">GameX {version ? `v${version}` : ""}</span>
      </footer>
    </div>
  );
}

/** A game's launcher icon, or a controller when it has none. */
function GameIcon({ icon, size }: { icon?: string | null; size: number }) {
  return (
    <span className="game-icon" style={{ width: size, height: size }}>
      {icon ? <img src={icon} alt="" /> : <Gamepad2 size={size * 0.55} />}
    </span>
  );
}
