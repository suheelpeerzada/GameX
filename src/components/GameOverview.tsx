import { Box, ChevronDown, Gamepad2, Hammer, Map, Music, Package, Palette, SlidersHorizontal, Users } from "lucide-react";
import { lazy, Suspense, type ReactNode } from "react";
import { formatBytes } from "../lib/api";
import type { Features } from "../lib/features";
import type { CapabilityStatus, Category, GameObject, GameProject, ImportRecord } from "../lib/types";

const ValueSliders = lazy(() => import("./ValueSliders"));

interface Props {
  project: ImportRecord;
  name: string;
  icon: string | null;
  analysis: GameProject | null;
  features: Features;
  onOpen: (objectId: string) => void;
  onEdit: () => void;
  onBuild: () => void;
  onExport: () => void;
  /** A gameplay value was changed (the game's files now differ). */
  onEdited: () => void;
}

/** What each status means, in words anyone reads the same way. */
const CAN: Record<CapabilityStatus, string> = { supported: "Yes", partial: "Partly", unsupported: "Not yet", unknown: "Not sure" };

/** What was found, in plain words, in the order people look for it. */
const FOUND: { category: Category; one: string; many: string; Icon: typeof Box }[] = [
  { category: "characters", one: "character", many: "characters", Icon: Users },
  { category: "map", one: "level or map", many: "levels and maps", Icon: Map },
  { category: "items", one: "item", many: "items", Icon: Package },
  { category: "assets", one: "picture or model", many: "pictures and models", Icon: Palette },
  { category: "audio", one: "sound", many: "sounds and music", Icon: Music },
  { category: "ui", one: "menu or text piece", many: "menus, buttons and text", Icon: Box },
];

/** How a game was made, as a sentence. */
function madeWith(analysis: GameProject): string {
  const e = analysis.engine;
  if (e.confidence < 0.5) return "GameX couldn't tell what this game was made with, so it shows its files as they are.";
  if (e.id === "android-native") return "An Android game written in Java or Kotlin.";
  return `Made with ${e.name}.`;
}

/**
 * The first thing shown for a game: what's in it, what can be changed, and
 * the common changes as cards that open the right place. The files and
 * the technical details stay one click away.
 */
export default function GameOverview({ project, name, icon, analysis, features, onOpen, onEdit, onBuild, onExport, onEdited }: Props) {
  const of = (category: Category) => analysis?.objects.filter((o) => o.category === category) ?? [];
  const firstWithLook = (list: GameObject[]) => list.find((o) => /image|sprite|texture|model/i.test(o.kind)) ?? list[0];
  const character = firstWithLook(of("characters"));
  const sound = of("audio")[0];
  const values = analysis?.objects.some((o) => o.gameplay.length > 0) ?? false;
  const others = analysis ? analysis.objects.length - FOUND.reduce((n, f) => n + of(f.category).length, 0) : 0;

  const cards: { title: string; detail: string; Icon: typeof Box; go?: () => void; why?: string }[] = [
    {
      title: "Change how a character looks",
      detail: "Swap a character's picture or model for your own.",
      Icon: Users,
      go: character ? () => onOpen(character.id) : undefined,
      why: "No characters were found in this game.",
    },
    {
      title: "Swap sounds",
      detail: "Replace music and sound effects.",
      Icon: Music,
      go: sound ? () => onOpen(sound.id) : undefined,
      why: "No sounds were found in this game.",
    },
    {
      title: "Make it easier or harder",
      detail: "Change speeds, health, damage and other numbers.",
      Icon: SlidersHorizontal,
      go: values ? () => document.getElementById("game-values")?.scrollIntoView({ behavior: "smooth" }) : undefined,
      why: "GameX hasn't found numbers it can change in this game yet. In Edit, each thing's numbers can be changed in your editable copy.",
    },
    {
      title: "Edit a level",
      detail: "Move, add or remove things in a 2D or 3D view, then play it.",
      Icon: Map,
      go: onEdit,
    },
    features.build
      ? { title: "Make a new APK", detail: "Build an installable copy of the game with your changes.", Icon: Hammer, go: onBuild }
      : { title: "Export a copy", detail: "Copy the game's files, with your changes, into a folder.", Icon: Hammer, go: onExport },
  ];

  return (
    <div className="screen overview">
      <section className="overview-hero">
        <span className="game-icon" style={{ width: 64, height: 64 }}>
          {icon ? <img src={icon} alt="" /> : <Gamepad2 size={34} />}
        </span>
        <div>
          <h1>{name}</h1>
          <p className="muted">{analysis ? madeWith(analysis) : "Looking through the game…"}</p>
        </div>
      </section>

      {analysis && (
        <>
          <section className="panel">
            <h2>What do you want to change?</h2>
            <div className="task-cards">
              {cards.map(({ title, detail, Icon, go, why }) => (
                <button key={title} className="task-card" onClick={go} disabled={!go} title={go ? detail : why}>
                  <Icon size={20} />
                  <span className="task-title">{title}</span>
                  <span className="muted small">{go ? detail : why}</span>
                </button>
              ))}
            </div>
          </section>

          <section className="panel">
            <h2>What's in it</h2>
            <ul className="found">
              {FOUND.map(({ category, one, many, Icon }) => {
                const n = of(category).length;
                return n ? (
                  <li key={category}>
                    <Icon size={16} /> <strong>{n.toLocaleString()}</strong> {n === 1 ? one : many}
                  </li>
                ) : null;
              })}
              {others > 0 && <li className="muted">and {others.toLocaleString()} code and setup files (under Files)</li>}
            </ul>
          </section>

          <section className="panel">
            <h2>What you can change</h2>
            <ul className="capabilities">
              {analysis.capabilities.map((c) => (
                <li key={c.feature}>
                  <span className={`status ${c.status}`}>{CAN[c.status]}</span>
                  <div>
                    <div>{c.label}</div>
                    <div className="muted small">{c.note}</div>
                  </div>
                </li>
              ))}
            </ul>
          </section>

          {values && (
            <Suspense fallback={null}>
              <ValueSliders projectId={project.id} analysis={analysis} onEdited={onEdited} />
            </Suspense>
          )}
        </>
      )}

      <Details title="Technical details">
        {analysis && (
          <>
            <h3>Engine</h3>
            <p>
              <strong>{analysis.engine.name}</strong>{" "}
              <span className="muted small">({Math.round(analysis.engine.confidence * 100)}% sure)</span>
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
          </>
        )}
        <h3>Unpacking</h3>
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
        <h3>Files</h3>
        <dl className="details">
          <dt>Imported from</dt>
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
          <dt>Working folder</dt>
          <dd className="mono">{project.workspacePath}</dd>
        </dl>
      </Details>
    </div>
  );
}

/** A section folded away until asked for. */
function Details({ title, children }: { title: string; children: ReactNode }) {
  return (
    <details className="panel overview-details">
      <summary>
        <ChevronDown size={15} /> {title}
      </summary>
      {children}
    </details>
  );
}
