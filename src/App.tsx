import { useCallback, useEffect, useState } from "react";
import logo from "./assets/logo.png";
import { api, containerLabel, inDesktopApp } from "./lib/api";
import type { ImportRecord } from "./lib/types";
import AboutScreen from "./screens/AboutScreen";
import ImportScreen from "./screens/ImportScreen";
import ProjectScreen from "./screens/ProjectScreen";
import SettingsScreen from "./screens/SettingsScreen";

type View = { kind: "import" } | { kind: "settings" } | { kind: "about" } | { kind: "project"; id: string };

export default function App() {
  const [view, setView] = useState<View>({ kind: "import" });
  const [projects, setProjects] = useState<ImportRecord[]>([]);
  // Projects opened this session stay mounted (hidden when not shown), so
  // tabs, AI answers and unsaved edits survive a trip to Settings.
  const [opened, setOpened] = useState<string[]>([]);
  // Focus mode hides the sidebar and the project's tree.
  const [focus, setFocus] = useState(false);

  useEffect(() => {
    if (view.kind === "project") {
      setOpened((ids) => (ids.includes(view.id) ? ids : [...ids, view.id]));
    }
  }, [view]);

  const refresh = useCallback(async () => {
    if (!inDesktopApp) return;
    setProjects(await api.listProjects());
  }, []);

  useEffect(() => {
    refresh().catch(console.error);
  }, [refresh]);


  const remove = async (project: ImportRecord) => {
    const ok = window.confirm(
      `Delete "${project.name}" from GameX?\n\nThis removes its workspace, including your edits and history. ` +
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

  return (
    <div className="app">
      <aside className="sidebar" hidden={focus && view.kind === "project"}>
        <div className="brand">
          <img src={logo} alt="" className="brand-logo" />
          GameX
        </div>
        <nav>
          <button
            className={view.kind === "import" ? "nav active" : "nav"}
            onClick={() => setView({ kind: "import" })}
          >
            Import a game
          </button>
          <button
            className={view.kind === "settings" ? "nav active" : "nav"}
            onClick={() => setView({ kind: "settings" })}
          >
            Settings
          </button>
          <button
            className={view.kind === "about" ? "nav active" : "nav"}
            onClick={() => setView({ kind: "about" })}
          >
            About
          </button>
        </nav>
        <div className="section-label">Projects</div>
        <ul className="project-list">
          {projects.length === 0 && <li className="muted small">Nothing imported yet.</li>}
          {projects.map((p) => (
            <li key={p.id} className="project-item">
              <button
                className={view.kind === "project" && view.id === p.id ? "nav active" : "nav"}
                onClick={() => setView({ kind: "project", id: p.id })}
              >
                <span className="project-name">{p.name}</span>
                <span className="badge">{containerLabel[p.container]}</span>
              </button>
              <button
                className="delete-project"
                title={`Delete ${p.name}`}
                aria-label={`Delete ${p.name}`}
                onClick={() => remove(p)}
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      </aside>
      <main className="content">
        {!inDesktopApp && (
          <div className="banner">
            You are viewing the UI in a browser. Importing needs the desktop app: run{" "}
            <code>npm run tauri dev</code>.
          </div>
        )}
        {view.kind === "import" && (
          <ImportScreen
            onImported={async (record) => {
              await refresh();
              setView({ kind: "project", id: record.id });
            }}
          />
        )}
        {view.kind === "settings" && <SettingsScreen />}
        {view.kind === "about" && <AboutScreen />}
        {projects
          .filter((p) => opened.includes(p.id))
          .map((p) => {
            const shown = view.kind === "project" && view.id === p.id;
            return (
              <div key={p.id} className="project-host" hidden={!shown}>
                <ProjectScreen
                  project={p}
                  active={shown}
                  focus={focus}
                  onToggleFocus={() => setFocus((f) => !f)}
                />
              </div>
            );
          })}
      </main>
    </div>
  );
}
