import { getVersion } from "@tauri-apps/api/app";
import { appDataDir } from "@tauri-apps/api/path";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import logo from "../assets/logo.png";
import { inDesktopApp } from "../lib/api";

const PORTFOLIO = "https://suheelpeerzada.github.io";
const REPOSITORY = "https://github.com/suheelpeerzada/GameX";

/** App information: version, author and where GameX keeps its data. */
export default function AboutScreen() {
  const [version, setVersion] = useState<string | null>(null);
  const [dataDir, setDataDir] = useState<string | null>(null);

  useEffect(() => {
    if (!inDesktopApp) return;
    getVersion().then(setVersion).catch(() => {});
    appDataDir().then(setDataDir).catch(() => {});
  }, []);

  // Links open in the default browser, not inside the app window.
  const link = (url: string, label: string) => (
    <a
      href={url}
      onClick={(e) => {
        e.preventDefault();
        openUrl(url).catch(() => window.open(url, "_blank"));
      }}
    >
      {label}
    </a>
  );

  return (
    <div className="screen">
      <section className="about-hero">
        <img src={logo} alt="" className="about-logo" />
        <div>
          <h1>GameX</h1>
          <p className="muted">{version ? `Version ${version}` : "Desktop app"}</p>
        </div>
      </section>

      <section className="panel">
        <p>
          GameX imports offline games (APK, ZIP or folder), recovers their content, and shows it in one categorized
          editor: Characters, Core, Map, Items, Assets, Audio, UI, Code and Unsorted. You can preview and edit files,
          rebuild and sign APKs, and, with your own API key, get AI help with any file.
        </p>
        <dl className="details">
          <dt>Created by</dt>
          <dd>{link(PORTFOLIO, "suheelpeerzada.github.io")}</dd>
          <dt>Source code</dt>
          <dd>{link(REPOSITORY, "github.com/suheelpeerzada/GameX")}</dd>
          {dataDir && (
            <>
              <dt>Your data</dt>
              <dd className="mono">{dataDir}</dd>
            </>
          )}
        </dl>
      </section>

      <section className="panel">
        <h2>Built with</h2>
        <p className="muted small">
          Tauri, React, TypeScript and Rust. When installed, GameX runs these separate open-source tools: apktool,
          jadx and uber-apk-signer (with Java). AI features use the provider of the key you add in Settings.
        </p>
      </section>
    </div>
  );
}
