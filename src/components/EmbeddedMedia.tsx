import { Download, Music, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { api, formatBytes, pickSaveFile } from "../lib/api";
import type { MediaItem } from "../lib/types";

const PAGE = 24;

/**
 * Images and sounds found inside a file by their content signatures, so media
 * can be seen and saved from any engine's container formats.
 */
export default function EmbeddedMedia({ projectId, path, auto }: { projectId: string; path: string; auto: boolean }) {
  const [items, setItems] = useState<MediaItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [shown, setShown] = useState(PAGE);

  const search = async () => {
    setBusy(true);
    setError(null);
    try {
      setItems(await api.scanMedia(projectId, path));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    setItems(null);
    setShown(PAGE);
    if (auto) search();
    // Re-run only when the file changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, path]);

  const save = async (item: MediaItem, index: number) => {
    const stem = (path.split("/").pop() ?? "media").replace(/\.[^.]+$/, "");
    const ext = item.format === "jpeg" ? "jpg" : item.format;
    const dest = await pickSaveFile(`${stem}_${index + 1}.${ext}`, item.format.toUpperCase(), ext);
    if (!dest) return;
    try {
      await api.saveMediaItem(projectId, path, item.offset, item.len, dest);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <section className="card embedded">
      <div className="embedded-head">
        <h4>Embedded media</h4>
        {items === null && (
          <button onClick={search} disabled={busy}>
            <Search size={14} />
            {busy ? "Searching…" : "Search this file"}
          </button>
        )}
      </div>
      <p className="muted small">
        Images and sounds found inside this file by their content (PNG, JPEG, GIF, BMP, WebP, WAV, OGG, MP3), whatever
        the engine.
      </p>
      {error && <div className="panel error small">{error}</div>}
      {items && items.length === 0 && <p className="muted small">Nothing found in this file.</p>}
      {items && items.length > 0 && (
        <>
          <p className="small">
            Found {items.filter((i) => i.kind === "image").length} images and{" "}
            {items.filter((i) => i.kind === "audio").length} sounds.
          </p>
          <div className="embedded-grid">
            {items.slice(0, shown).map((item, i) => (
              <EmbeddedItem
                key={item.offset}
                projectId={projectId}
                path={path}
                item={item}
                onSave={() => save(item, i)}
              />
            ))}
          </div>
          {items.length > shown && (
            <button className="link" onClick={() => setShown((n) => n + PAGE)}>
              Show {Math.min(PAGE, items.length - shown)} more of {items.length - shown}
            </button>
          )}
        </>
      )}
    </section>
  );
}

function EmbeddedItem(props: { projectId: string; path: string; item: MediaItem; onSave: () => void }) {
  const { projectId, path, item } = props;
  const [src, setSrc] = useState<string | null>(null);
  const load = () =>
    api
      .readMediaItem(projectId, path, item.offset, item.len, item.mime)
      .then(setSrc)
      .catch(() => {});

  // Images load right away; sounds when asked, as they can be large.
  useEffect(() => {
    if (item.kind === "image") load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item.offset]);

  const size = item.width && item.height ? `${item.width}×${item.height} · ` : "";
  return (
    <div className="embedded-item">
      {item.kind === "image" ? (
        <span className="thumb">{src && <img src={src} alt="" />}</span>
      ) : src ? (
        <audio controls autoPlay src={src} />
      ) : (
        <button className="embedded-play" onClick={load}>
          <Music size={16} />
          Play
        </button>
      )}
      <div className="embedded-meta">
        <span>
          {item.format.toUpperCase()} · {size}
          {formatBytes(item.len)}
        </span>
        <button className="icon-button small-icon" onClick={props.onSave} title="Save as a file">
          <Download size={14} />
        </button>
      </div>
    </div>
  );
}
