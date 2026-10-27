import { useEffect, useState } from "react";
import { api, inDesktopApp } from "../lib/api";
import type { AiStatus, ModelInfo } from "../lib/types";

/** API key and model for the optional AI assistant. */
export default function AiSettings() {
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!inDesktopApp) return;
    api.aiStatus().then((s) => {
      setStatus(s);
      if (s.provider) api.aiModels().then(setModels).catch((e) => setError(String(e)));
    });
  }, []);

  const saveKey = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.aiSetKey(key);
      setStatus(result.status);
      setModels(result.models);
      setKey("");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!window.confirm("Remove the stored API key from this PC?")) return;
    setError(null);
    try {
      setStatus(await api.aiClearKey());
      setModels([]);
    } catch (e) {
      setError(String(e));
    }
  };

  const chooseModel = async (model: string) => {
    setError(null);
    try {
      setStatus(await api.aiSetModel(model));
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <section className="panel">
      <h2>
        AI assistant{" "}
        {status && (
          <span className={status.configured ? "pill ok" : "pill warn"}>{status.configured ? "ready" : "off"}</span>
        )}
      </h2>
      <p className="muted small">
        Optional. With a key, the inspector can explain a file or selection, show where it's used, and change its
        values. Paste a key from Anthropic (sk-ant-…), OpenAI (sk-…), OpenRouter (sk-or-…), Google Gemini (AIza…) or Groq (gsk_…):
        the provider is detected from the key, and you pick from the models it can use.
      </p>
      <p className="muted small">
        Usage is billed by the provider. A Claude Pro subscription doesn't include API access: create a key at
        console.anthropic.com. Files are sent only when you click Explain, Ask or Propose. The key is stored in Windows
        Credential Manager, not in a file.
      </p>

      {status?.provider ? (
        <>
          <dl className="details">
            <dt>Provider</dt>
            <dd>{status.providerLabel}</dd>
            <dt>Key</dt>
            <dd className="mono">{status.keyHint}</dd>
            <dt>Model</dt>
            <dd>
              <select value={status.model ?? ""} onChange={(e) => chooseModel(e.target.value)}>
                {!status.model && <option value="">Choose a model</option>}
                {status.model && !models.some((m) => m.id === status.model) && (
                  <option value={status.model}>{status.model}</option>
                )}
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name === m.id ? m.id : `${m.name} (${m.id})`}
                  </option>
                ))}
              </select>
            </dd>
          </dl>
          <div className="row ai-settings-actions">
            <button onClick={remove}>Remove key</button>
          </div>
        </>
      ) : (
        <div className="row">
          <input
            type="password"
            placeholder="Paste an API key"
            value={key}
            autoComplete="off"
            spellCheck={false}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && key.trim() && !busy && saveKey()}
          />
          <button className="primary" disabled={!inDesktopApp || busy || !key.trim()} onClick={saveKey}>
            {busy ? "Checking…" : "Save key"}
          </button>
        </div>
      )}
      {error && <p className="small error-text">{error}</p>}
    </section>
  );
}
