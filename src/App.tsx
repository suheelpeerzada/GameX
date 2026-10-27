import { useCallback, useEffect, useState } from "react";
import { api, containerLabel, inDesktopApp } from "./lib/api";
import type { ImportRecord } from "./lib/types";
import ImportScreen from "./screens/ImportScreen";
import ProjectScreen from "./screens/ProjectScreen";
import SettingsScreen from "./screens/SettingsScreen";

type View = { kind: "import" } | { kind: "settings" } | { kind: "project"; id: string };

export default function App() {
  const [view, setView] = useState<View>({ kind: "import" });
  const [projects, setProjects] = useState<ImportRecord[]>([]);

  const refresh = useCallback(async () => {
    if (!inDesktopApp) return;
    setProjects(await api.listProjects());
  }, []);

  useEffect(() => {
    refresh().catch(console.error);
  }, [refresh]);

  const current = view.kind === "project" ? projects.find((p) => p.id === view.id) : undefined;

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">GameX</div>
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
        </nav>
        <div className="section-label">Projects</div>
        <ul className="project-list">
          {projects.length === 0 && <li className="muted small">Nothing imported yet.</li>}
          {projects.map((p) => (
            <li key={p.id}>
              <button
                className={view.kind === "project" && view.id === p.id ? "nav active" : "nav"}
                onClick={() => setView({ kind: "project", id: p.id })}
              >
                <span className="project-name">{p.name}</span>
                <span className="badge">{containerLabel[p.container]}</span>
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
        {view.kind === "project" && current && <ProjectScreen project={current} />}
      </main>
    </div>
  );
}
