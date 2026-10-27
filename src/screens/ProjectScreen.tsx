import { containerLabel, formatBytes } from "../lib/api";
import type { ImportRecord } from "../lib/types";

export default function ProjectScreen({ project }: { project: ImportRecord }) {
  const importedAt = new Date(project.importedAt * 1000).toLocaleString();
  return (
    <div className="screen">
      <h1>{project.name}</h1>
      <p className="muted">
        {containerLabel[project.container]} imported {importedAt}
      </p>

      <section className="panel">
        <h2>Unpacking</h2>
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
      </section>

      <section className="panel">
        <h2>Details</h2>
        <dl className="details">
          <dt>Source</dt>
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
          <dt>Workspace</dt>
          <dd className="mono">{project.workspacePath}</dd>
        </dl>
      </section>

      <section className="panel muted">
        <h2>Analysis</h2>
        <p>
          Engine detection, the categorized view and the capability report arrive in the next
          milestone.
        </p>
      </section>
    </div>
  );
}
