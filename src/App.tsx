import { getVersion } from "@tauri-apps/api/app";
import { ChevronRight, Download, Gamepad2, Info, Settings } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import logo from "./assets/logo.png";
import { api, inDesktopApp } from "./lib/api";
import type { ImportRecord } from "./lib/types";
import { containerLongLabel } from "./lib/visuals";
import ImportScreen from "./screens/ImportScreen";
import ProjectScreen from "./screens/ProjectScreen";

// Loaded when first visited.
const AboutScreen = lazy(() => import("./screens/AboutScreen"));
const SettingsScreen = lazy(() => import("./screens/SettingsScreen"));

type View = { kind: "import" } | { kind: "settings" } | { kind: "about" } | { kind: "project"; id: string };

export default function App() {
  const [view, setView] = useState<View>({ kind: "import" });
  const [projects, setProjects] = useState<ImportRecord[]>([]);
  const [version, setVersion] = useState<string | null>(null);
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
    if (inDesktopApp) getVersion().then(setVersion).catch(() => {});
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

  const navButton = (kind: "import" | "settings", label: string, Icon: typeof Download) => (
    <button className={view.kind === kind ? "nav active" : "nav"} onClick={() => setView({ kind })}>
      <Icon size={18} />
      <span>{label}</span>
    </button>
  );

  return (
    <div className="app">
      <aside className="sidebar" hidden={focus && view.kind === "project"}>
        <div className="brand">
          <img src={logo} alt="" className="brand-logo" />
          <span>GameX</span>
        </div>
        <nav className="main-nav">
          {navButton("import", "Import", Download)}
          {navButton("settings", "Settings", Settings)}
        </nav>
        <div className="section-label">Projects</div>
        <ul className="project-list">
          {projects.length === 0 && <li className="muted small">Nothing imported yet.</li>}
          {projects.map((p) => {
            const active = view.kind === "project" && view.id === p.id;
            return (
              <li key={p.id} className="project-item">
                <button
                  className={active ? "project-card active" : "project-card"}
                  onClick={() => setView({ kind: "project", id: p.id })}
                >
                  <span className="project-card-icon">
                    <Gamepad2 size={20} />
                  </span>
                  <span className="project-card-text">
                    <span className="project-name">{p.name}</span>
                    <span className="project-kind">{containerLongLabel[p.container]}</span>
                  </span>
                  <ChevronRight size={16} className="project-card-chevron" />
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
            );
          })}
        </ul>
        <button
          className={view.kind === "about" ? "sidebar-footer active" : "sidebar-footer"}
          onClick={() => setView({ kind: "about" })}
          title="About GameX"
        >
          <Info size={16} />
          <span>{version ? `v${version}` : "About"}</span>
        </button>
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
        <Suspense fallback={null}>
          {view.kind === "settings" && <SettingsScreen />}
          {view.kind === "about" && <AboutScreen />}
        </Suspense>
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
                  onOpenSettings={() => setView({ kind: "settings" })}
                />
              </div>
            );
          })}
      </main>
    </div>
  );
}
