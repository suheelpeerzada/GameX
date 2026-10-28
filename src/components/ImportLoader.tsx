import { useEffect, useState } from "react";
import logo from "../assets/logo.png";
import type { ImportProgress } from "../lib/types";

/** Short labels for the import stages (src-tauri/src/import.rs, extract.rs). */
const STAGES: Record<string, string> = {
  detect: "Detecting",
  original: "Securing original",
  extract: "Extracting",
  copy: "Copying",
  raw: "Unpacking",
  apktool: "Decoding",
  jadx: "Decompiling",
  unity: "Unity assets",
  godot: "Godot project",
  done: "Finishing",
};

const TICKS = 60;

/** Full-window loader while a game is imported: the logo in a spinning ring, and the current step only. */
export default function ImportLoader({ current, name }: { current: ImportProgress | null; name: string }) {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    const start = Date.now();
    const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - start) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const elapsed = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
  const stage = current ? (STAGES[current.stage] ?? current.stage) : "Starting";

  return (
    <div className="import-loader" role="status" aria-live="polite">
      <div className="loader-ring">
        <svg viewBox="0 0 200 200" aria-hidden="true">
          <defs>
            <linearGradient id="loader-arc" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor="#22d3ee" />
              <stop offset="55%" stopColor="#7465ff" />
              <stop offset="100%" stopColor="#e040fb" />
            </linearGradient>
          </defs>
          <circle className="loader-track" cx="100" cy="100" r="86" />
          <g className="loader-ticks">
            {Array.from({ length: TICKS }, (_, i) => (
              <line
                key={i}
                x1="100"
                y1="4"
                x2="100"
                y2={i % 5 === 0 ? 12 : 8}
                transform={`rotate(${(360 / TICKS) * i} 100 100)`}
              />
            ))}
          </g>
          <circle className="loader-arc" cx="100" cy="100" r="86" stroke="url(#loader-arc)" />
          <circle className="loader-arc inner" cx="100" cy="100" r="74" stroke="url(#loader-arc)" />
        </svg>
        <img className="loader-logo" src={logo} alt="" />
      </div>
      <div className="loader-name">{name}</div>
      <div className="loader-stage">
        <span className="loader-dot" />
        {stage}
        <span className="loader-time">{elapsed}</span>
      </div>
      {/* Keyed so each new step fades in. */}
      <div key={current?.message ?? ""} className="loader-message">
        {current?.message ?? "Preparing the import…"}
      </div>
    </div>
  );
}
