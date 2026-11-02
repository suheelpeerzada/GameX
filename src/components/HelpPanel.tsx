import { HelpCircle, X } from "lucide-react";
import { useEffect, useState } from "react";

/** Short how-tos, in the order people need them. */
const HOW: { q: string; a: string }[] = [
  {
    q: "Open a game",
    a: "Drop the game's file on the window, or click Import game. An Android game is the .apk file you'd install on a phone. GameX works on a copy: the file you picked is never changed.",
  },
  {
    q: "See what's in it",
    a: "A game opens on Overview: what was found (characters, levels, sounds) and what GameX can change in it. The cards under \"What do you want to change?\" take you to the right place.",
  },
  {
    q: "Change a picture or a sound",
    a: "On Overview pick \"Change how a character looks\" or \"Swap sounds\", then Replace and choose your file. Undo puts the old one back.",
  },
  {
    q: "Make it easier or harder",
    a: "On Overview, under \"Make it easier or harder\", drag a slider (speed, health, damage...). Reset puts the game's own number back. Build a new APK to try it on a phone or emulator.",
  },
  {
    q: "Edit a level",
    a: "Edit opens the game's levels in a 2D or 3D view. Click something to select it, then move (W), turn (E) or resize (R) it. Everything saves as you go, and Undo (Ctrl+Z) and Redo (Ctrl+Y) work across restarts.",
  },
  {
    q: "Play what you changed",
    a: "Press Play (F5). GameX plays your edited copy with its own simple rules, not the original game's code, so some things behave differently. Shift+F5 stops.",
  },
  {
    q: "Run the real game",
    a: "Run original installs the game on an emulator (or a phone with USB debugging) and runs it. For Java and Kotlin games you can also watch and change its objects while it runs.",
  },
  {
    q: "Make a new APK",
    a: "Make a new APK (Build) asks for a name and an icon, then builds an installable copy with your changes. Switch on \"Install next to the original game\" to keep both on the phone.",
  },
];

const KEYS: { area: string; keys: [string, string][] }[] = [
  {
    area: "Everywhere",
    keys: [
      ["F1", "This help"],
      ["Ctrl+Z / Ctrl+Y", "Undo / Redo"],
    ],
  },
  {
    area: "Edit",
    keys: [
      ["Q / W / E / R", "Select / Move / Turn / Resize"],
      ["F", "Frame the selection"],
      ["Del", "Delete"],
      ["Ctrl+D / Ctrl+G", "Duplicate / Group"],
      ["Right mouse + WASD, Q/E", "Fly the camera (Shift faster)"],
      ["Arrows, PgUp/PgDn", "Move the camera"],
      ["Middle drag / scroll", "Pan / Zoom"],
    ],
  },
  {
    area: "Play",
    keys: [
      ["F5", "Play"],
      ["Shift+F5", "Stop"],
      ["Ctrl+Shift+F5", "Restart"],
      ["F6 / F10", "Pause / One frame"],
      ["F8", "Free camera"],
      ["Ctrl+[ / Ctrl+0 / Ctrl+]", "Slower / Normal / Faster"],
    ],
  },
];

/** Opens the help from anywhere (a button, an empty screen). */
export function openHelp() {
  window.dispatchEvent(new Event("gamex:help"));
}

/** The help panel: how-tos and the shortcut sheet. F1 opens and closes it. */
export default function HelpPanel() {
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F1") {
        e.preventDefault();
        setOpen((o) => !o);
      } else if (e.key === "Escape") setOpen(false);
    };
    const onAsk = () => setOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener("gamex:help", onAsk);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("gamex:help", onAsk);
    };
  }, []);

  if (!open) return null;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && setOpen(false)}>
      <div className="modal help-panel" role="dialog" aria-modal="true" aria-label="Help">
        <div className="help-head">
          <h2>
            <HelpCircle size={18} /> Help
          </h2>
          <button className="icon-button" onClick={() => setOpen(false)} title="Close (F1 or Esc)">
            <X size={16} />
          </button>
        </div>
        <div className="help-body">
          <section>
            <h3>How to</h3>
            {HOW.map(({ q, a }) => (
              <details key={q}>
                <summary>{q}</summary>
                <p>{a}</p>
              </details>
            ))}
          </section>
          <section>
            <h3>Keys</h3>
            {KEYS.map(({ area, keys }) => (
              <div key={area} className="help-keys">
                <h4>{area}</h4>
                <dl>
                  {keys.map(([k, what]) => (
                    <div key={k}>
                      <dt>
                        <kbd>{k}</kbd>
                      </dt>
                      <dd>{what}</dd>
                    </div>
                  ))}
                </dl>
              </div>
            ))}
          </section>
        </div>
      </div>
    </div>
  );
}
