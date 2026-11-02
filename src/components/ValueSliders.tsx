import { RotateCcw, SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/api";
import { friendlyError } from "../lib/errors";
import type { GameProject } from "../lib/types";
import ErrorText from "./ErrorText";

interface Props {
  projectId: string;
  analysis: GameProject;
  onEdited: () => void;
}

/** One number the game starts with, where it's written, and what it was. */
interface Tunable {
  key: string;
  owner: string;
  label: string;
  field: string;
  original: string;
  /** The smali file it's set in (found when the panel opens). */
  file: string | null;
}

/** `jumpPower` / `JUMP_POWER` -> "Jump power". */
function words(field: string): string {
  const s = field
    .replace(/^m(?=[A-Z])/, "")
    .replace(/_/g, " ")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .trim()
    .toLowerCase();
  return s.charAt(0).toUpperCase() + s.slice(1);
}

/** A slider's range around the game's own value: from zero (or below it) to about three times it. */
function range(original: number): { min: number; max: number; step: number; whole: boolean } {
  const whole = Number.isInteger(original);
  const size = Math.max(Math.abs(original) * 3, whole ? 10 : 1);
  const min = original < 0 ? -size : 0;
  const step = whole ? 1 : Number((size / 200).toPrecision(1));
  return { min, max: size, step, whole };
}

/**
 * "Make it easier or harder": the speeds, health, damage and other numbers
 * the analyzer found in the game's code, each as a slider starting at the
 * game's own value, with Reset to original. Changes go into the game's code,
 * so a new APK has them; each is one undo step.
 */
export default function ValueSliders({ projectId, analysis, onEdited }: Props) {
  const tunables = useMemo(() => {
    const out: (Tunable & { candidates: string[] })[] = [];
    for (const o of analysis.objects) {
      const smali = o.sourceFiles.filter((f) => f.endsWith(".smali"));
      for (const g of o.gameplay) {
        if (!/^-?\d+(\.\d+)?f?$|^(true|false)$/.test(g.value.trim())) continue;
        out.push({ key: `${o.id}:${g.name}`, owner: o.displayName, label: words(g.name), field: g.name, original: g.value.replace(/f$/, ""), file: null, candidates: smali });
      }
    }
    return out;
  }, [analysis]);
  const [found, setFound] = useState<Record<string, { file: string; now: string }>>({});
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<string | null>(null);

  // Where each value is set in the code, and what it is now (after earlier changes).
  useEffect(() => {
    const items: [string, string][] = [];
    const owners: { key: string; file: string }[] = [];
    for (const t of tunables) {
      for (const file of t.candidates) {
        items.push([file, t.field]);
        owners.push({ key: t.key, file });
      }
    }
    if (!items.length) return;
    api
      .gameplayNow(projectId, items)
      .then((values) => {
        const next: Record<string, { file: string; now: string }> = {};
        values.forEach((v, i) => {
          if (v !== null && !next[owners[i].key]) next[owners[i].key] = { file: owners[i].file, now: v };
        });
        setFound(next);
      })
      .catch(() => {});
  }, [projectId, tunables]);

  const shown = tunables.filter((t) => found[t.key]);
  if (!shown.length) return null;

  const save = async (t: Tunable, value: string) => {
    const at = found[t.key];
    if (!at || value === at.now) return;
    setSaving(t.key);
    setError(null);
    try {
      await api.setGameplayValue(projectId, at.file, t.field, value);
      setFound((f) => ({ ...f, [t.key]: { ...at, now: value } }));
      onEdited();
    } catch (e) {
      setError(friendlyError(e).message);
    } finally {
      setSaving(null);
      setDraft((d) => {
        const { [t.key]: _, ...rest } = d;
        return rest;
      });
    }
  };

  // Grouped by what they belong to (the player, an enemy...).
  const groups = new Map<string, Tunable[]>();
  for (const t of shown) groups.set(t.owner, [...(groups.get(t.owner) ?? []), t]);

  return (
    <section className="panel" id="game-values">
      <h2>
        <SlidersHorizontal size={18} /> Make it easier or harder
      </h2>
      <p className="muted small">
        The numbers this game starts with. Drag a slider, then build a new APK (or Run original) to try it. Reset puts the game's own number back.
      </p>
      {error && <p className="bad small"><ErrorText text={error} /></p>}
      {[...groups].map(([owner, list]) => (
        <div key={owner} className="value-group">
          <h3>{owner}</h3>
          {list.map((t) => {
            const now = draft[t.key] ?? found[t.key].now;
            const original = Number(t.original);
            const yesNo = /^(true|false)$/.test(t.original);
            const r = range(original);
            const changed = found[t.key].now !== t.original;
            return (
              <div key={t.key} className={changed ? "value-row changed" : "value-row"}>
                <span className="value-name" title={`${t.field} (the game's own value: ${t.original})`}>
                  {t.label}
                </span>
                {yesNo ? (
                  <input type="checkbox" checked={now === "true"} disabled={saving === t.key} onChange={(e) => save(t, String(e.target.checked))} />
                ) : (
                  <>
                    <input
                      type="range"
                      min={Math.min(r.min, Number(now))}
                      max={Math.max(r.max, Number(now))}
                      step={r.step}
                      value={Number(now)}
                      disabled={saving === t.key}
                      onChange={(e) => setDraft((d) => ({ ...d, [t.key]: e.target.value }))}
                      onPointerUp={(e) => save(t, (e.target as HTMLInputElement).value)}
                      onKeyUp={(e) => save(t, (e.target as HTMLInputElement).value)}
                    />
                    <input
                      className="value-number"
                      type="number"
                      step={r.whole ? 1 : "any"}
                      value={now}
                      disabled={saving === t.key}
                      onChange={(e) => setDraft((d) => ({ ...d, [t.key]: e.target.value }))}
                      onBlur={(e) => e.target.value !== "" && save(t, r.whole ? String(Math.round(Number(e.target.value))) : e.target.value)}
                    />
                  </>
                )}
                <button className="icon-button" disabled={!changed || saving === t.key} onClick={() => save(t, t.original)} title={`Reset to the game's own value (${t.original})`}>
                  <RotateCcw size={14} />
                </button>
              </div>
            );
          })}
        </div>
      ))}
    </section>
  );
}
