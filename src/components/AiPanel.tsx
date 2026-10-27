import { useEffect, useState } from "react";
import { api } from "../lib/api";
import type { AiStatus, Answer, EditState, Explanation, Proposal, TokenUsage } from "../lib/types";
import Markdown from "./Markdown";

interface Props {
  projectId: string;
  /** The file shown in the inspector (relative to unpacked/). */
  path: string;
  /** Text the user selected in the preview, if any. */
  selection: string;
  /** Changes when the key or model may have changed in Settings. */
  refreshKey: number;
  onEdited: (state: EditState) => void;
}

/**
 * Optional AI help for one file: explain it (or the selected part), show
 * where it's used, list values to tune, answer questions, and propose edits
 * that are only saved after the user approves them.
 */
export default function AiPanel({ projectId, path, selection, refreshKey, onEdited }: Props) {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [busy, setBusy] = useState<null | "explain" | "ask" | "propose" | "apply">(null);
  const [error, setError] = useState<string | null>(null);
  const [explanation, setExplanation] = useState<Explanation | null>(null);
  const [newValues, setNewValues] = useState<Record<string, string>>({});
  const [instruction, setInstruction] = useState("");
  const [proposal, setProposal] = useState<Proposal | null>(null);
  const [applied, setApplied] = useState<string | null>(null);
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState<Answer | null>(null);
  // The selection the current explanation was made for, reused for proposals.
  const [scope, setScope] = useState<string | null>(null);

  useEffect(() => {
    api.aiStatus().then(setStatus).catch(() => setStatus(null));
  }, [refreshKey]);

  const sel = selection.trim() ? selection : null;

  const run = async <T,>(kind: NonNullable<typeof busy>, action: () => Promise<T>): Promise<T | null> => {
    setBusy(kind);
    setError(null);
    try {
      return await action();
    } catch (e) {
      setError(String(e));
      return null;
    } finally {
      setBusy(null);
    }
  };

  const explain = async () => {
    const result = await run("explain", () => api.aiExplain(projectId, path, sel));
    if (!result) return;
    setExplanation(result);
    setScope(sel);
    setNewValues({});
    setProposal(null);
    setApplied(null);
  };

  const ask = async () => {
    const result = await run("ask", () => api.aiAsk(projectId, path, sel, question));
    if (result) setAnswer(result);
  };

  const propose = async () => {
    if (!explanation) return;
    const changes = explanation.values
      .filter((v) => (newValues[v.id] ?? "").trim() !== "" && newValues[v.id].trim() !== v.current)
      .map((v) => ({ id: v.id, name: v.name, current: v.current, newValue: newValues[v.id].trim() }));
    const result = await run("propose", () => api.aiPropose(projectId, path, scope, changes, instruction));
    if (result) {
      setProposal(result);
      setApplied(null);
    }
  };

  const apply = async () => {
    if (!proposal) return;
    await run("apply", async () => {
      // Refuse if the file changed after the AI read it.
      const current = await api.readTextFile(projectId, proposal.target);
      if (current !== proposal.original) {
        throw new Error("The file changed after the AI read it. Propose the change again.");
      }
      onEdited(await api.saveTextFile(projectId, proposal.target, proposal.updated));
      setApplied(proposal.summary);
      setProposal(null);
    });
  };

  if (!status) return null;
  if (!status.configured) {
    return (
      <section className="ai-panel">
        <h3>AI assistant</h3>
        <p className="muted small">
          Optional. Add an API key under Settings → AI assistant to have this file explained, see where it's used,
          and change its values.
        </p>
      </section>
    );
  }

  const changedCount = explanation
    ? explanation.values.filter((v) => (newValues[v.id] ?? "").trim() !== "" && newValues[v.id].trim() !== v.current)
        .length
    : 0;

  return (
    <section className="ai-panel">
      <div className="ai-head">
        <h3>AI assistant</h3>
        <span className="muted small">
          {status.providerLabel} · {status.model}
        </span>
      </div>

      <div className="row">
        <button className="primary" onClick={explain} disabled={busy !== null}>
          {busy === "explain" ? "Explaining…" : sel ? "Explain selection" : "Explain this file"}
        </button>
        <span className="muted small">
          {sel
            ? `Using the ${sel.split("\n").length}-line selection.`
            : "Tip: select part of the preview to ask about just that part."}{" "}
          The file is sent to {status.providerLabel} when you click.
        </span>
      </div>

      {error && <div className="panel error small">{error}</div>}

      {explanation && (
        <div className="ai-result">
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
                  onClick={propose}
                  disabled={busy !== null || (changedCount === 0 && instruction.trim() === "")}
                >
                  {busy === "propose" ? "Writing the change…" : "Propose change"}
                </button>
                <span className="muted small">
                  Changes go to <span className="mono">{explanation.editTarget}</span>. You'll see them before
                  anything is saved.
                </span>
              </div>
            </>
          ) : (
            <p className="muted small">
              There's no editable version of this file, so the AI can explain it but not change it.
            </p>
          )}
          <Tokens usage={explanation.tokens} model={explanation.model} trimmed={explanation.trimmed} />
        </div>
      )}

      {proposal && (
        <div className="ai-proposal">
          <p>
            <strong>Proposed:</strong> {proposal.summary}
          </p>
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
          <div className="row">
            <button className="primary" onClick={apply} disabled={busy !== null}>
              {busy === "apply" ? "Applying…" : "Apply change"}
            </button>
            <button onClick={() => setProposal(null)} disabled={busy !== null}>
              Discard
            </button>
            <span className="muted small">Applying saves it as an edit you can undo. Rebuild the APK to try it.</span>
          </div>
          <Tokens usage={proposal.tokens} model={proposal.model} trimmed={proposal.trimmed} />
        </div>
      )}
      {applied && <div className="notice small">Applied: {applied} Use Undo to revert it.</div>}

      <div className="ai-ask">
        <div className="row">
          <input
            type="text"
            placeholder={sel ? "Ask about the selection…" : "Ask about this file…"}
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && question.trim() && busy === null && ask()}
          />
          <button onClick={ask} disabled={busy !== null || !question.trim()}>
            {busy === "ask" ? "Thinking…" : "Ask"}
          </button>
        </div>
        {answer && (
          <div className="ai-result">
            <Markdown text={answer.answer} />
            <Tokens usage={answer.tokens} model={answer.model} trimmed={answer.trimmed} />
          </div>
        )}
      </div>
    </section>
  );
}

function Tokens({ usage, model, trimmed }: { usage: TokenUsage; model: string; trimmed: string | null }) {
  return (
    <div className="muted small ai-tokens">
      {trimmed && <div>{trimmed}</div>}
      {model} · {usage.input.toLocaleString()} tokens in, {usage.output.toLocaleString()} out
    </div>
  );
}
