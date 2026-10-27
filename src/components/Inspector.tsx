import { ChevronDown, ChevronLeft, ChevronRight, ChevronUp } from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { api, extensionOf, formatBytes, isEditablePath, mediaExtensions, pickFile } from "../lib/api";
import { categories, categoryLabel } from "../lib/categories";
import type { Category, EditState, GameObject, Preview, References } from "../lib/types";
import { CategoryIcon, ObjectIcon, UNEDITABLE_FORMATS, fileType, isImagePath } from "../lib/visuals";
import { isModelPath } from "../lib/models";
import EmbeddedMedia from "./EmbeddedMedia";
import SceneEditor from "./SceneEditor";

type Section = "details" | "properties" | "code" | "references";

interface Props {
  projectId: string;
  object: GameObject;
  edited: Set<string>;
  /** Goes up after every edit, so previews reload. */
  revision: number;
  /** Objects with related names, shown under Details. */
  related: GameObject[];
  /** Opens another object in a tab. */
  onOpen: (id: string) => void;
  onMove: (category: Category | null) => void;
  onEdited: (state: EditState) => void;
  /** Reports the file shown and the selected text, for the AI group. */
  onContext: (file: string | null, selection: string) => void;
}

const TEXT_FILE = /\.(java|smali|kt|xml|json|ya?ml|txt|csv|ini|cfg|lua|js|ts|py|gd|cs|c|cpp|h|glsl|md)$/i;

export default function Inspector(props: Props) {
  const { projectId, object, edited, revision, related, onOpen, onMove, onEdited, onContext } = props;
  // The parent keeps one Inspector per tab, so this state lasts per tab.
  const [file, setFile] = useState(object.sourceFiles[0] ?? null);
  // A line to scroll to in the preview (from a gameplay value).
  const [focusLine, setFocusLine] = useState<number | null>(null);
  const [section, setSection] = useState<Section>("details");
  // For .tscn files: the visual scene editor or the plain text.
  const [sceneView, setSceneView] = useState<"scene" | "text">("scene");
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Text selected in the preview or editor, for "Explain selection".
  const [selection, setSelection] = useState("");
  const previewRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    onContext(file, selection);
    // onContext is a new function on every parent render; only changes matter.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file, selection]);

  const captureSelection = () => {
    const sel = window.getSelection();
    const inside = sel && previewRef.current && sel.anchorNode && previewRef.current.contains(sel.anchorNode);
    setSelection(inside ? sel.toString() : "");
  };

  const run = async (action: () => Promise<EditState>) => {
    setError(null);
    try {
      onEdited(await action());
    } catch (e) {
      setError(String(e));
    }
  };

  /** Shows a file in the Code tab, optionally at a line. */
  const choose = (f: string, edit = false, line: number | null = null) => {
    setFocusLine(line);
    setFile(f);
    setEditing(edit);
    setError(null);
    setSelection("");
    setSection("code");
  };

  const codeLabel = object.sourceFiles.some((f) => TEXT_FILE.test(f)) ? "Code" : "File";
  const sections: [Section, string][] = [
    ["details", "Details"],
    ["properties", "Properties"],
    ["code", codeLabel],
    ["references", "References"],
  ];

  return (
    <div className="inspector">
      <MediaCard projectId={projectId} object={object} revision={revision} onPick={(f) => choose(f)} />

      <div className="section-tabs" role="tablist">
        {sections.map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={section === id}
            className={section === id ? "section-tab active" : "section-tab"}
            onClick={() => setSection(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {section === "details" && (
        <DetailsSection
          object={object}
          edited={edited}
          related={related}
          onMove={onMove}
          onPick={(f, line) => choose(f, false, line ?? null)}
          onOpen={onOpen}
        />
      )}

      {section === "properties" &&
        (object.properties.length > 0 ? (
          <dl className="details card-body">
            {object.properties.map((p) => (
              <Detail key={p.label} label={p.label} value={p.value} />
            ))}
          </dl>
        ) : (
          <p className="muted small">No extra properties were recorded for this object.</p>
        ))}

      {section === "code" && (
        <>
          <ul className="file-pills">
            {object.sourceFiles.map((f) => {
              const { label, Icon } = fileType(f);
              return (
                <li key={f}>
                  <button
                    className={f === file ? "file-pill selected" : "file-pill"}
                    onClick={() => choose(f)}
                    title={f}
                  >
                    <Icon size={13} />
                    <span>{f.split("/").pop()}</span>
                    <span className="file-pill-meta">
                      {label} · {f.split("/")[0]}
                    </span>
                    {edited.has(f) && <span className="tag">edited</span>}
                    {!isEditablePath(f) && <span className="tag muted-tag">read-only</span>}
                  </button>
                </li>
              );
            })}
          </ul>
          {file && (
            <FileActions
              object={object}
              file={file}
              edited={edited.has(file)}
              editing={editing}
              onEdit={() => setEditing(true)}
              onChoose={(f) => choose(f, true)}
              onReplace={async () => {
                const ext = extensionOf(file);
                const source = await pickFile(`.${ext} file`, [ext]);
                if (source) await run(() => api.replaceFile(projectId, file, source));
              }}
              onRevert={() => run(() => api.revertFile(projectId, file))}
            />
          )}
          {error && <div className="panel error small">{error}</div>}
          {file && file.endsWith(".tscn") && !editing && (
            <div className="segmented scene-switch">
              <button className={sceneView === "scene" ? "active" : ""} onClick={() => setSceneView("scene")}>
                Scene
              </button>
              <button className={sceneView === "text" ? "active" : ""} onClick={() => setSceneView("text")}>
                Text
              </button>
            </div>
          )}
          {file && file.endsWith(".tscn") && !editing && sceneView === "scene" ? (
            <SceneEditor
              projectId={projectId}
              path={file}
              editable={isEditablePath(file)}
              revision={revision}
              onEdited={onEdited}
            />
          ) : file &&
            (editing ? (
              <TextEditor
                projectId={projectId}
                path={file}
                onSelectText={setSelection}
                onCancel={() => setEditing(false)}
                onSaved={(state) => {
                  onEdited(state);
                  setEditing(false);
                }}
              />
            ) : (
              <div ref={previewRef} onMouseUp={captureSelection} onKeyUp={captureSelection}>
                <FilePreview
                  key={`${file}:${revision}`}
                  projectId={projectId}
                  path={file}
                  focusLine={focusLine}
                  onEdited={onEdited}
                />
              </div>
            ))}
          {file && !editing && mightHoldMedia(file) && (
            <EmbeddedMedia
              projectId={projectId}
              path={file}
              auto={object.properties.some((p) => p.label === "Embedded media")}
            />
          )}
        </>
      )}

      {section === "references" && <ReferencesSection projectId={projectId} object={object} onOpen={onOpen} />}
    </div>
  );
}

/** Binary files that aren't themselves an image or sound may contain some. */
function mightHoldMedia(path: string): boolean {
  return !TEXT_FILE.test(path) && !isImagePath(path) && !mediaExtensions.has(extensionOf(path));
}

/** One copy per file name: the best-decoded copy comes first in sourceFiles. */
function uniqueByName(files: string[]): string[] {
  const seen = new Set<string>();
  return files.filter((f) => {
    const name = f.split("/").pop() ?? f;
    if (seen.has(name)) return false;
    seen.add(name);
    return true;
  });
}

const FRAMES_PER_PAGE = 8;

/** The card at the top: frames for images, otherwise a summary. */
function MediaCard(props: { projectId: string; object: GameObject; revision: number; onPick: (f: string) => void }) {
  const { projectId, object, revision, onPick } = props;
  const [page, setPage] = useState(0);
  const frames = uniqueByName(object.sourceFiles.filter(isImagePath));
  const pages = Math.ceil(frames.length / FRAMES_PER_PAGE);
  const prop = (label: string) => object.properties.find((p) => p.label === label)?.value;
  const meta = [
    fileType(object.sourceFiles[0] ?? "").label,
    prop("Frames") && `${prop("Frames")} frames`,
    prop("Dimensions")?.replace(" × ", "×"),
    prop("File size"),
    !frames.length && `${uniqueByName(object.sourceFiles).length} file${object.sourceFiles.length === 1 ? "" : "s"}`,
  ].filter(Boolean);

  return (
    <section className="media-card">
      {frames.length > 0 ? (
        <>
          <div className="gallery">
            {pages > 1 && (
              <button
                className="gallery-arrow"
                aria-label="Previous frames"
                onClick={() => setPage((p) => (p - 1 + pages) % pages)}
              >
                <ChevronLeft size={18} />
              </button>
            )}
            <div className="frames">
              {frames.slice(page * FRAMES_PER_PAGE, (page + 1) * FRAMES_PER_PAGE).map((f) => (
                <button key={f} className="frame" onClick={() => onPick(f)} title={f}>
                  <Thumbnail projectId={projectId} path={f} revision={revision} />
                  <span className="frame-label">{(f.split("/").pop() ?? f).replace(/\.[^.]+$/, "")}</span>
                </button>
              ))}
            </div>
            {pages > 1 && (
              <button className="gallery-arrow" aria-label="Next frames" onClick={() => setPage((p) => (p + 1) % pages)}>
                <ChevronRight size={18} />
              </button>
            )}
          </div>
          {pages > 1 && (
            <div className="dots">
              {Array.from({ length: pages }, (_, i) => (
                <span key={i} className={i === page ? "dot active" : "dot"} />
              ))}
            </div>
          )}
        </>
      ) : (
        <div className="media-summary-icon">
          <ObjectIcon object={object} size={28} />
        </div>
      )}
      <div className="media-caption">
        <div className="media-title">
          <CategoryIcon category={object.category} size={13} />
          <span>{object.displayName}</span>
        </div>
        <div className="muted small">{meta.join("  •  ")}</div>
      </div>
    </section>
  );
}

function Thumbnail({ projectId, path, revision }: { projectId: string; path: string; revision: number }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    api
      .readPreview(projectId, path)
      .then((p) => current && p.type === "image" && setSrc(p.dataUrl))
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [projectId, path, revision]);
  return <span className="thumb">{src && <img src={src} alt="" />}</span>;
}

function DetailsSection(props: {
  object: GameObject;
  edited: Set<string>;
  related: GameObject[];
  onMove: (category: Category | null) => void;
  onPick: (f: string, line?: number) => void;
  onOpen: (id: string) => void;
}) {
  const { object, edited, related, onMove, onPick, onOpen } = props;
  const [relatedOpen, setRelatedOpen] = useState(true);
  const moved = object.category !== object.autoCategory;
  const confidence = Math.round(object.confidence * 100);
  const confidenceColor = confidence >= 70 ? "var(--ok)" : confidence >= 45 ? "var(--warn)" : "var(--bad)";

  return (
    <>
      <div className="details-grid">
        <dl className="info-list">
          <dt>Name</dt>
          <dd>{object.displayName}</dd>
          <dt>Kind</dt>
          <dd>{object.kind}</dd>
          {object.role && (
            <>
              <dt>Role</dt>
              <dd>
                <span className="role-badge">{object.role}</span>
              </dd>
            </>
          )}
          <dt>Category</dt>
          <dd>
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
          </dd>
          <dt>Confidence</dt>
          <dd className="confidence-row">
            <span style={{ color: confidenceColor }}>{confidence}%</span>
            <span className="bar">
              <span style={{ width: `${confidence}%`, background: confidenceColor }} />
            </span>
          </dd>
          <dt>Reason</dt>
          <dd className="small">
            {moved ? (
              <>
                You moved this from {categoryLabel[object.autoCategory]}. GameX had placed it there because{" "}
                {object.reason}.{" "}
                <button className="link" onClick={() => onMove(null)}>
                  Undo
                </button>
              </>
            ) : (
              <>{object.reason[0]?.toUpperCase() + object.reason.slice(1)}.</>
            )}
          </dd>
        </dl>

        <div className="card source-files">
          <h4>Source Files</h4>
          <ul>
            {object.sourceFiles.map((f) => {
              const { label, Icon } = fileType(f);
              return (
                <li key={f}>
                  <button onClick={() => onPick(f)} title={f}>
                    <Icon size={14} className="object-icon" />
                    <span className="source-name">{f.split("/").pop()}</span>
                    {edited.has(f) && <span className="edited-dot" title="Edited" />}
                    <span className="source-type">{label}</span>
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      </div>

      {object.gameplay.length > 0 && (
        <div className="card gameplay">
          <h4>Gameplay values</h4>
          <p className="muted small">
            Numbers this code sets, found where they're written. Click one to see it in the file.
          </p>
          <ul>
            {object.gameplay.map((g) => (
              <li key={`${g.name}:${g.line}`}>
                <button onClick={() => onPick(g.file, g.line)} title={`${g.file}, line ${g.line}`}>
                  <span className="gameplay-name">{g.name}</span>
                  <span className="gameplay-value mono">{g.value}</span>
                  <span className="muted small">
                    {g.setIn === "constructor" ? "set when created" : "initial value"} · line {g.line}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {related.length > 0 && (
        <div className="card related">
          <button className="related-head" onClick={() => setRelatedOpen((o) => !o)}>
            {relatedOpen ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
            <span>Related Objects</span>
          </button>
          {relatedOpen && (
            <ul>
              {related.map((r) => (
                <li key={r.id}>
                  <button onClick={() => onOpen(r.id)}>
                    <ObjectIcon object={r} size={15} />
                    <span className="related-name">{r.displayName}</span>
                    <span className="related-kind">{r.kind}</span>
                    <ChevronRight size={15} className="muted" />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </>
  );
}

/** Where other classes use this one, found in smali (no AI needed). */
function ReferencesSection(props: { projectId: string; object: GameObject; onOpen: (id: string) => void }) {
  const { projectId, object, onOpen } = props;
  const classFile = object.sourceFiles.find((f) => f.endsWith(".java") || f.endsWith(".smali"));
  const [refs, setRefs] = useState<References | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!classFile) return;
    api
      .getReferences(projectId, classFile)
      .then(setRefs)
      .catch((e) => setError(String(e)));
  }, [projectId, classFile]);

  if (!classFile) return <p className="muted small">References are found for code classes only.</p>;
  if (error) return <div className="panel error small">{error}</div>;
  if (!refs) return <p className="muted small">Searching the game's code…</p>;
  if (refs.items.length === 0) return <p className="muted small">No other class refers to this one directly.</p>;

  return (
    <div className="card references">
      <p className="muted small">
        Used in {refs.items.length + refs.more} place{refs.items.length + refs.more === 1 ? "" : "s"} across the
        game's code.
      </p>
      <ul>
        {refs.items.map((r, i) => {
          const outer = r.class.split("$")[0];
          const simple = outer.split(".").pop();
          return (
            <li key={i}>
              <button onClick={() => onOpen(`class:${outer}`)} title={`Open ${outer}`}>
                <span className="ref-class">{simple}</span>
                {r.method && <span className="muted small"> in {r.method}()</span>}
                <code className="ref-line">{r.line}</code>
              </button>
            </li>
          );
        })}
      </ul>
      {refs.more > 0 && <p className="muted small">…and {refs.more} more.</p>}
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

  if (UNEDITABLE_FORMATS.has(extensionOf(file))) {
    return (
      <div className="notice small">
        This is {fileType(file).label === "File" ? "an exported engine file" : `a ${fileType(file).label.toLowerCase()}`} in the
        engine's own exported format. GameX can show it, but editing or replacing this format isn't supported yet.
      </div>
    );
  }

  if (isModelPath(file)) {
    // Replacing happens in the 3D view, which can convert and fit a model.
    const text = TEXT_MODELS.has(extensionOf(file));
    return (
      <div className="row file-actions">
        {text && <button onClick={props.onEdit}>Edit as text</button>}
        {props.edited && (
          <button onClick={props.onRevert} title="Go back to the game's original version (can be undone)">
            Restore original
          </button>
        )}
        <span className="muted small">Use Replace in the 3D view to swap in another model.</span>
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
  onSelectText: (text: string) => void;
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
        onSelect={(e) => {
          const el = e.currentTarget;
          props.onSelectText(el.value.slice(el.selectionStart, el.selectionEnd));
        }}
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

/** Model formats that are plain text, so they can also be edited as text. */
const TEXT_MODELS = new Set(["obj", "gltf", "dae"]);

// three.js is large, so the 3D view loads only when a model is opened.
const ModelView = lazy(() => import("./ModelView"));

function FilePreview({
  projectId,
  path,
  focusLine,
  onEdited,
}: {
  projectId: string;
  path: string;
  focusLine?: number | null;
  onEdited?: (state: EditState) => void;
}) {
  const [preview, setPreview] = useState<Preview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const preRef = useRef<HTMLPreElement>(null);

  // Scroll a text preview so the given line is near the top.
  useEffect(() => {
    const pre = preRef.current;
    if (!pre || !focusLine || preview?.type !== "text") return;
    const lineHeight = parseFloat(getComputedStyle(pre).lineHeight) || 16;
    pre.scrollTop = Math.max(0, (focusLine - 3) * lineHeight);
  }, [preview, focusLine]);

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
          <pre ref={preRef} className="code">
            {preview.text}
          </pre>
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
    case "model":
      return (
        <Suspense fallback={<div className="preview muted">Starting the 3D view…</div>}>
          <ModelView
            projectId={projectId}
            path={path}
            format={preview.format}
            size={preview.size}
            editable={isEditablePath(path)}
            onEdited={onEdited}
          />
        </Suspense>
      );
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
