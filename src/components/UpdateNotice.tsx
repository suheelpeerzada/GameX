import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Download } from "lucide-react";
import { useEffect, useState } from "react";
import { inDesktopApp } from "../lib/api";

/**
 * Looks for a newer GameX once at start (installed copies only) and offers it
 * in the status bar: one click downloads it, installs it and restarts.
 */
export default function UpdateNotice() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [state, setState] = useState<"ready" | "getting" | "failed">("ready");
  const [percent, setPercent] = useState(0);

  useEffect(() => {
    // `tauri dev` has nothing to update.
    if (!inDesktopApp || import.meta.env.DEV) return;
    check()
      .then((found) => found && setUpdate(found))
      // Offline, or no release yet: say nothing.
      .catch(() => {});
  }, []);

  if (!update) return null;

  const install = async () => {
    setState("getting");
    let total = 0;
    let got = 0;
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          got += event.data.chunkLength;
          if (total) setPercent(Math.round((100 * got) / total));
        }
      });
      await relaunch();
    } catch {
      setState("failed");
    }
  };

  return (
    <button className="update-notice" onClick={install} disabled={state === "getting"} title={update.body ?? `GameX ${update.version}`}>
      <Download size={13} />
      {state === "getting"
        ? `Getting GameX ${update.version}… ${percent ? `${percent}%` : ""}`
        : state === "failed"
          ? "The update didn't install. Try again"
          : `GameX ${update.version} is ready: update and restart`}
    </button>
  );
}
