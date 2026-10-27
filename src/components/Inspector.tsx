import { useEffect, useRef, useState } from "react";
import { api, extensionOf, formatBytes, isEditablePath, mediaExtensions, pickFile } from "../lib/api";
import { categories, categoryLabel } from "../lib/categories";
import type { Category, EditState, GameObject, Preview } from "../lib/types";

interface Props {
  projectId: string;
  object: GameObject;
  edited: Set<string>;
  /** Goes up after every edit, so previews reload. */
  revision: number;
  onMove: (category: Category | null) => void;
  onEdited: (state: EditState) => void;
}

export default function Inspector({ projectId, object, edited, revision, onMove, onEdited }: Props) {
  // The parent keys this component by object id, so this resets per object.
  const [file, setFile] = useState(object.sourceFiles[0] ?? null);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const moved = object.category !== object.autoCategory;

  const run = async (action: () => Promise<EditState>) => {
    setError(null);
    try {
      onEdited(await action());
    } catch (e) {
      setError(String(e));
    }
  };

  const choose = (f: string) => {
    setFile(f);
    setEditing(false);
    setError(null);
  };

  return (
    <div className="inspector">
      <div className="inspector-head">
        <div>
          <h2>{object.displayName}</h2>
          <div className="muted">{object.kind}</div>
        </div>
        <label className="move">
          <span className="muted small">Category</span>
          <select
            value={object.category}
            onChange={(e) => {
              const value = e.target.value as Category;
              onMove(value === object.autoCategory ? null : value);
            }}
          >
            {categories.map((c) => (
              <option key={c.id} value={c.id}>
                {c.label}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className={moved ? "why moved" : "why"}>
        {moved ? (
          <>
            You moved this to {categoryLabel[object.category]}. GameX had put it in{" "}
            {categoryLabel[object.autoCategory]} because {object.reason}.{" "}
            <button className="link" onClick={() => onMove(null)}>
              Undo
            </button>
          </>
        ) : (
          <>
            In {categoryLabel[object.category]} because {object.reason}.
            <span className="confidence" title="How sure the analyzer is">
              <span className="bar">
                <span style={{ width: `${Math.round(object.confidence * 100)}%` }} />
              </span>
              {Math.round(object.confidence * 100)}% sure
            </span>
          </>
        )}
      </div>

      {object.properties.length > 0 && (
        <dl className="details">
          {object.properties.map((p) => (
            <Detail key={p.label} label={p.label} value={p.value} />
          ))}
        </dl>
      )}

      <h3>Files ({object.sourceFiles.length})</h3>
      <ul className="files">
        {object.sourceFiles.map((f) => (
          <li key={f}>
            <button className={f === file ? "file selected mono" : "file mono"} onClick={() => choose(f)}>
              {f}
              {edited.has(f) && <span className="tag">edited</span>}
              {!isEditablePath(f) && <span className="tag muted-tag">read-only</span>}
            </button>
          </li>
        ))}
      </ul>

      {file && (
        <FileActions
          object={object}
          file={file}
          edited={edited.has(file)}
          editing={editing}
          onEdit={() => setEditing(true)}
          onChoose={(f) => {
            choose(f);
            setEditing(true);
          }}
          onReplace={async () => {
            const ext = extensionOf(file);
            const source = await pickFile(`.${ext} file`, [ext]);
            if (source) await run(() => api.replaceFile(projectId, file, source));
          }}
          onRevert={() => run(() => api.revertFile(projectId, file))}
        />
      )}
      {error && <div className="panel error small">{error}</div>}

      {file &&
        (editing ? (
          <TextEditor
            projectId={projectId}
            path={file}
            onCancel={() => setEditing(false)}
            onSaved={(state) => {
              onEdited(state);
              setEditing(false);
            }}
          />
        ) : (
          <FilePreview key={`${file}:${revision}`} projectId={projectId} path={file} />
        ))}
    </div>
  );
}

function Detail({ label, value }: { label: string; value: string }) {
  return (
    <>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </>
  );
}

function FileActions(props: {
  object: GameObject;
  file: string;
  edited: boolean;
  editing: boolean;
  onEdit: () => void;
  onChoose: (file: string) => void;
  onReplace: () => void;
  onRevert: () => void;
}) {
  const { object, file, edited, editing } = props;
  if (editing) return null;

  if (!isEditablePath(file)) {
    // Decompiled Java can't be rebuilt; the class's smali can.
    const smali = object.sourceFiles.find((f) => f.endsWith(".smali") && isEditablePath(f));
    const twin = object.sourceFiles.find((f) => f !== file && isEditablePath(f));
    return (
      <div className="notice small">
        {file.startsWith("jadx/") && file.endsWith(".java") ? (
          <>
            This is decompiled Java, shown for reading. It can't be rebuilt, so changes go in the class's smali
            file, which is what the game is rebuilt from.
          </>
        ) : (
          <>This copy is read-only. The game is rebuilt from apktool's copy.</>
        )}{" "}
        {smali ? (
          <button onClick={() => props.onChoose(smali)}>Edit the smali</button>
        ) : (
          twin && <button onClick={() => props.onChoose(twin)}>Open the editable copy</button>
        )}
      </div>
    );
  }

  const media = mediaExtensions.has(extensionOf(file));
  return (
    <div className="row file-actions">
      {media ? (
        <button onClick={props.onReplace}>Replace…</button>
      ) : (
        <button onClick={props.onEdit}>Edit</button>
      )}
      {edited && (
        <button onClick={props.onRevert} title="Go back to the game's original version (can be undone)">
          Restore original
        </button>
      )}
      {media && <span className="muted small">The new file must be a .{extensionOf(file)} file.</span>}
    </div>
  );
}

function TextEditor(props: {
  projectId: string;
  path: string;
  onCancel: () => void;
  onSaved: (state: EditState) => void;
}) {
  const { projectId, path } = props;
  const [original, setOriginal] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    api
      .readTextFile(projectId, path)
      .then((t) => {
        setOriginal(t);
        setText(t);
      })
      .catch((e) => setError(String(e)));
  }, [projectId, path]);

  const dirty = original !== null && text !== original;

  const save = async () => {
    if (!dirty || saving) return;
    setSaving(true);
    setError(null);
    try {
      props.onSaved(await api.saveTextFile(projectId, path, text));
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };

  const cancel = () => {
    if (dirty && !window.confirm("Discard your unsaved changes?")) return;
    props.onCancel();
  };

  if (error && original === null) {
    return (
      <div className="panel error small">
        {error} <button onClick={props.onCancel}>Back</button>
      </div>
    );
  }
  if (original === null) return <div className="preview muted">Loading…</div>;

  return (
    <div className="editor">
      <div className="row">
        <button className="primary" onClick={save} disabled={!dirty || saving}>
          {saving ? "Saving…" : "Save"}
        </button>
        <button onClick={cancel}>Cancel</button>
        <span className="muted small">
          {dirty ? "Unsaved changes · Ctrl+S to save" : "No changes"} · the original is kept, and saving can be undone
        </span>
      </div>
      {error && <div className="panel error small">{error}</div>}
      <textarea
        ref={area}
        className="code-editor"
        value={text}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "s" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            save();
          } else if (e.key === "Tab" && !e.shiftKey) {
            // Indent instead of leaving the editor.
            e.preventDefault();
            const el = e.currentTarget;
            const { selectionStart: start, selectionEnd: end } = el;
            setText(text.slice(0, start) + "    " + text.slice(end));
            requestAnimationFrame(() => el.setSelectionRange(start + 4, start + 4));
          }
        }}
      />
    </div>
  );
}

function FilePreview({ projectId, path }: { projectId: string; path: string }) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    setPreview(null);
    setError(null);
    api
      .readPreview(projectId, path)
      .then((p) => current && setPreview(p))
      .catch((e) => current && setError(String(e)));
    return () => {
      current = false;
    };
  }, [projectId, path]);

  if (error) return <div className="preview muted">Couldn't open this file: {error}</div>;
  if (!preview) return <div className="preview muted">Loading preview…</div>;

  switch (preview.type) {
    case "image":
      return (
        <div className="preview image">
          <PixelImage src={preview.dataUrl} />
        </div>
      );
    case "audio":
      return (
        <div className="preview">
          <audio controls src={preview.dataUrl} />
        </div>
      );
    case "text":
      return (
        <div className="preview">
          <pre className="code">{preview.text}</pre>
          {preview.truncated && (
            <div className="muted small">Showing the start of a {formatBytes(preview.size)} file.</div>
          )}
        </div>
      );
    case "binary":
      return (
        <div className="preview">
          <div className="muted small">Binary file, {formatBytes(preview.size)}. First bytes:</div>
          <pre className="code">{preview.head}</pre>
        </div>
      );
    case "tooLarge":
      return <div className="preview muted">Too large to preview ({formatBytes(preview.size)}).</div>;
  }
}

/** Small sprites are scaled up with crisp pixels so they're visible. */
function PixelImage({ src }: { src: string }) {
  const [width, setWidth] = useState<number | null>(null);
  const scale = width && width < 160 ? Math.min(8, Math.floor(160 / width)) : 1;
  return (
    <img
      src={src}
      alt=""
      onLoad={(e) => setWidth(e.currentTarget.naturalWidth)}
      style={width ? { width: width * scale } : undefined}
      className={scale > 1 ? "pixelated" : undefined}
    />
  );
}
