import {
  ChevronRight,
  FileText,
  Link,
  MessageSquare,
  Pencil,
  Send,
  Sparkles,
  TextSelect,
  WandSparkles,
  type LucideIcon,
} from "lucide-react";
import { useRef, useState } from "react";
import { api } from "../lib/api";
import type { AiStatus, Answer, EditState, Explanation, Proposal, TokenUsage } from "../lib/types";
import Markdown from "./Markdown";

interface Props {
  projectId: string;
  /** The file in the active tab (relative to unpacked/), or null if none. */
  path: string | null;
  /** Text the user selected in that file, if any. */
  selection: string;
  status: AiStatus | null;
  onEdited: (state: EditState) => void;
  onOpenSettings: () => void;
}

type Kind = "explain" | "selection" | "usage" | "ask" | "propose";

/** One finished request, kept in the Recent list. */
interface Entry {
  id: number;
  kind: Kind;
  title: string;
  snippet: string;
  seconds: number;
  model: string;
  tokens: TokenUsage;
  result: { explanation: Explanation; scope: string | null } | { answer: Answer } | { proposal: Proposal };
}

const KIND_ICON: Record<Kind, LucideIcon> = {
  explain: FileText,
  selection: TextSelect,
  usage: Link,
  ask: MessageSquare,
  propose: WandSparkles,
};

const USAGE_QUESTION =
  "Where and how is this used in the game? List the places that use it and what happens there, based on the references.";

let nextId = 1;

/**
 * Optional AI help for one file: explain it (or the selected part), show where
 * it's used, change its values, and answer questions. Proposed edits are only
 * saved after the user approves them.
 */
export default function AiPanel({ projectId, path, selection, status, onEdited, onOpenSettings }: Props) {
  const [busy, setBusy] = useState<Kind | "apply" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<Entry[]>([]);
  const [currentId, setCurrentId] = useState<number | null>(null);
  const [question, setQuestion] = useState("");
  const [newValues, setNewValues] = useState<Record<string, string>>({});
  const [instruction, setInstruction] = useState("");
  const [applied, setApplied] = useState<string | null>(null);
  const questionRef = useRef<HTMLTextAreaElement>(null);
  const changeRef = useRef<HTMLDivElement>(null);

  if (!status?.configured) {
    return (
      <div className="ai-empty">
        <Sparkles size={22} className="ai-mark" />
        <p>
          Add an API key to have files explained, see where they're used, and change their values with AI. Anthropic,
          OpenAI, OpenRouter, Gemini and Groq keys work.
        </p>
        <button className="primary" onClick={onOpenSettings}>
          Open AI settings
        </button>
      </div>
    );
  }
  if (!path) return null;

  const fileName = path.split("/").pop() ?? path;
  const sel = selection.trim() ? selection : null;
  const current = history.find((e) => e.id === currentId) ?? null;
  // The latest explanation, whose values feed "Change it".
  const lastExplain = history.find((e) => "explanation" in e.result);
  const explanation = lastExplain && "explanation" in lastExplain.result ? lastExplain.result : null;

  const record = (entry: Omit<Entry, "id">) => {
    const full = { ...entry, id: nextId++ };
    setHistory((h) => [full, ...h].slice(0, 20));
    setCurrentId(full.id);
    return full;
  };

  const run = async <T,>(kind: Kind | "apply", action: () => Promise<T>): Promise<{ value: T; seconds: number } | null> => {
    setBusy(kind);
    setError(null);
    const started = performance.now();
    try {
      const value = await action();
      return { value, seconds: Math.max(1, Math.round((performance.now() - started) / 1000)) };
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setBusy(null);
    }
  };

  const explain = async (scope: string | null) => {
    const kind: Kind = scope ? "selection" : "explain";
    const r = await run(kind, () => api.aiExplain(projectId, path, scope));
    if (!r) return null;
    setNewValues({});
    setApplied(null);
    return record({
      kind,
      title: scope ? "Explain selection" : "Explain this file",
      snippet: r.value.summary,
      seconds: r.seconds,
      model: r.value.model,
      tokens: r.value.tokens,
      result: { explanation: r.value, scope },
    });
  };

  const ask = async (kind: Kind, text: string, title: string) => {
    const r = await run(kind, () => api.aiAsk(projectId, path, sel, text));
    if (!r) return;
    record({
      kind,
      title,
      snippet: r.value.answer.replace(/[#*`>-]/g, "").trim(),
      seconds: r.seconds,
      model: r.value.model,
      tokens: r.value.tokens,
      result: { answer: r.value },
    });
  };

  const sendQuestion = async () => {
    const text = question.trim();
    if (!text || busy) return;
    await ask("ask", text, text);
    setQuestion("");
  };

  const changeIt = async () => {
    if (!explanation) {
      const entry = await explain(sel);
      if (!entry) return;
    } else if (lastExplain) {
      setCurrentId(lastExplain.id);
    }
    requestAnimationFrame(() => changeRef.current?.scrollIntoView({ behavior: "smooth", block: "start" }));
  };

  const propose = async (source: Explanation, scope: string | null) => {
    const changes = source.values
      .filter((v) => (newValues[v.id] ?? "").trim() !== "" && newValues[v.id].trim() !== v.current)
      .map((v) => ({ id: v.id, name: v.name, current: v.current, newValue: newValues[v.id].trim() }));
    const r = await run("propose", () => api.aiPropose(projectId, path, scope, changes, instruction));
    if (!r) return;
    setApplied(null);
    record({
      kind: "propose",
      title: "Propose a change",
      snippet: `${r.value.summary} ${r.value.edits.length} edit${r.value.edits.length === 1 ? "" : "s"} in ${
        r.value.target.split("/").pop()
      }.`,
      seconds: r.seconds,
      model: r.value.model,
      tokens: r.value.tokens,
      result: { proposal: r.value },
    });
  };

  const apply = async (proposal: Proposal) => {
    await run("apply", async () => {
      // Refuse if the file changed after the AI read it.
      const now = await api.readTextFile(projectId, proposal.target);
      if (now !== proposal.original) {
        throw new Error("The file changed after the AI read it. Propose the change again.");
      }
      onEdited(await api.saveTextFile(projectId, proposal.target, proposal.updated));
      setApplied(proposal.summary);
    });
  };

  const action = (kind: Kind | "change", label: string, Icon: LucideIcon, onClick: () => void, disabled = false) => (
    <button
      className={current?.kind === kind ? "ai-action active" : "ai-action"}
      onClick={onClick}
      disabled={busy !== null || disabled}
    >
      <Icon size={17} />
      <span>{busy === kind ? "Working…" : label}</span>
    </button>
  );

  return (
    <div className="ai-panel">
      <div className="reading-card" title={path}>
        <span className="reading-icon">
          <FileText size={16} />
        </span>
        <span className="reading-text">
          Reading <strong>{fileName}</strong>
          {sel && <span className="muted small"> · {sel.split("\n").length}-line selection</span>}
        </span>
      </div>

      <div className="ai-actions">
        {action("explain", "Explain this file", FileText, () => explain(null))}
        {action("selection", "Explain selection", TextSelect, () => explain(sel), !sel)}
        {action("usage", "Where it's used", Link, () => ask("usage", USAGE_QUESTION, "Where is this used?"))}
        {action("change", "Change it", Pencil, changeIt)}
        {action("ask", "Ask", MessageSquare, () => questionRef.current?.focus())}
      </div>

      <div className="ask-box">
        <textarea
          ref={questionRef}
          rows={3}
          placeholder={sel ? "Ask about the selection…" : `Ask a question about ${fileName}…`}
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              sendQuestion();
            }
          }}
        />
        <button
          className="send-button"
          onClick={sendQuestion}
          disabled={busy !== null || !question.trim()}
          aria-label="Send question"
        >
          <Send size={16} />
        </button>
      </div>
      <p className="muted tiny">
        {fileName} is sent to {status.providerLabel} only when you click. Enter sends, Shift+Enter adds a line.
      </p>

      {error && <div className="panel error small">{error}</div>}
      {busy && busy !== "apply" && <div className="ai-working">Waiting for {status.model}…</div>}

      {current && (
        <div className="ai-result card">
          <div className="ai-result-title">{current.title}</div>
          {"explanation" in current.result && (
            <ExplanationView
              explanation={current.result.explanation}
              scope={current.result.scope}
              newValues={newValues}
              setNewValues={setNewValues}
              instruction={instruction}
              setInstruction={setInstruction}
              busy={busy !== null}
              proposing={busy === "propose"}
              changeRef={changeRef}
              onPropose={propose}
            />
          )}
          {"answer" in current.result && <Markdown text={current.result.answer.answer} />}
          {"proposal" in current.result && (
            <ProposalView
              proposal={current.result.proposal}
              applying={busy === "apply"}
              applied={applied}
              onApply={apply}
            />
          )}
          <Meta entry={current} />
        </div>
      )}

      {history.length > 0 && (
        <>
          <h3 className="recent-title">Recent</h3>
          <ul className="recent">
            {history.map((e) => {
              const Icon = KIND_ICON[e.kind];
              return (
                <li key={e.id}>
                  <button className={e.id === currentId ? "recent-item active" : "recent-item"} onClick={() => setCurrentId(e.id)}>
                    <span className="recent-icon">
                      <Icon size={17} />
                    </span>
                    <span className="recent-body">
                      <span className="recent-head">{e.title}</span>
                      <span className="recent-snippet">{e.snippet}</span>
                      <Meta entry={e} />
                    </span>
                    <ChevronRight size={16} className="muted" />
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </div>
  );
}

function Meta({ entry }: { entry: Entry }) {
  const trimmed =
    "explanation" in entry.result
      ? entry.result.explanation.trimmed
      : "answer" in entry.result
        ? entry.result.answer.trimmed
        : entry.result.proposal.trimmed;
  const total = entry.tokens.input + entry.tokens.output;
  return (
    <span className="recent-meta">
      {trimmed && <span className="trimmed-note">{trimmed}</span>}
      {entry.seconds}s · {entry.model} · {total >= 1000 ? `${(total / 1000).toFixed(1)}k` : total} tokens
    </span>
  );
}

function ExplanationView(props: {
  explanation: Explanation;
  scope: string | null;
  newValues: Record<string, string>;
  setNewValues: (f: (v: Record<string, string>) => Record<string, string>) => void;
  instruction: string;
  setInstruction: (s: string) => void;
  busy: boolean;
  proposing: boolean;
  changeRef: React.RefObject<HTMLDivElement | null>;
  onPropose: (e: Explanation, scope: string | null) => void;
}) {
  const { explanation, scope, newValues, setNewValues, instruction, setInstruction } = props;
  const changed = explanation.values.filter(
    (v) => (newValues[v.id] ?? "").trim() !== "" && newValues[v.id].trim() !== v.current,
  ).length;

  return (
    <>
      <p className="ai-summary">
        <strong>{explanation.summary}</strong>
      </p>
      <Markdown text={explanation.explanation} />

      {explanation.usage.length > 0 && (
        <>
          <h4 className="md-heading">Where it's used</h4>
          <ul className="ai-usage">
            {explanation.usage.map((u, i) => (
              <li key={i}>
                <span className="mono">{u.where}</span>: {u.how}
              </li>
            ))}
          </ul>
        </>
      )}

      <div ref={props.changeRef}>
        <h4 className="md-heading">Change it</h4>
        {explanation.editTarget ? (
          <>
            {explanation.values.length > 0 ? (
              <table className="ai-values">
                <thead>
                  <tr>
                    <th>Value</th>
                    <th>Now</th>
                    <th>New</th>
                  </tr>
                </thead>
                <tbody>
                  {explanation.values.map((v) => (
                    <tr key={v.id}>
                      <td>
                        <div>{v.name}</div>
                        <div className="muted small">{v.description}</div>
                      </td>
                      <td className="mono">{v.current}</td>
                      <td>
                        {v.type === "boolean" ? (
                          <select
                            value={newValues[v.id] ?? ""}
                            onChange={(e) => setNewValues((s) => ({ ...s, [v.id]: e.target.value }))}
                          >
                            <option value="">keep</option>
                            <option value="true">true</option>
                            <option value="false">false</option>
                          </select>
                        ) : (
                          <input
                            type="text"
                            className="mono"
                            placeholder="keep"
                            inputMode={v.type === "text" ? "text" : "decimal"}
                            value={newValues[v.id] ?? ""}
                            onChange={(e) => setNewValues((s) => ({ ...s, [v.id]: e.target.value }))}
                          />
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <p className="muted small">No tunable values were found here. You can still describe a change.</p>
            )}
            <textarea
              className="ai-instruction"
              placeholder="Or describe a change, e.g. “make the player jump twice as high”"
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              rows={2}
            />
            <div className="row">
              <button
                className="primary"
                onClick={() => props.onPropose(explanation, scope)}
                disabled={props.busy || (changed === 0 && instruction.trim() === "")}
              >
                {props.proposing ? "Writing the change…" : "Propose change"}
              </button>
              <span className="muted small">
                Edits go to <span className="mono">{explanation.editTarget.split("/").pop()}</span>; you'll review
                them first.
              </span>
            </div>
          </>
        ) : (
          <p className="muted small">There's no editable version of this file, so the AI can explain it but not change it.</p>
        )}
      </div>
    </>
  );
}

function ProposalView(props: {
  proposal: Proposal;
  applying: boolean;
  applied: string | null;
  onApply: (p: Proposal) => void;
}) {
  const { proposal } = props;
  const done = props.applied === proposal.summary;
  return (
    <>
      <p>{proposal.summary}</p>
      {proposal.notes.trim() && <Markdown text={proposal.notes} />}
      {proposal.edits.map((edit, i) => (
        <div key={i} className="diff">
          <div className="muted small">
            Change {i + 1} of {proposal.edits.length} in <span className="mono">{proposal.target}</span>
          </div>
          <pre className="code diff-old">{edit.find}</pre>
          <pre className="code diff-new">{edit.replace}</pre>
        </div>
      ))}
      {done ? (
        <div className="notice small">Applied. Use Undo to revert it, and Build APK to try it.</div>
      ) : (
        <div className="row">
          <button className="primary" onClick={() => props.onApply(proposal)} disabled={props.applying}>
            {props.applying ? "Applying…" : "Apply change"}
          </button>
          <span className="muted small">Saved as an edit you can undo.</span>
        </div>
      )}
    </>
  );
}
