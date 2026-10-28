import { ChevronDown, ChevronUp, FileAudio, Folder, Image, Package, Play, Plus, Replace, Search, Shapes, Square, Type } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Thing, ThingKind } from "../../lib/gx";

/** Asset types, by the folder a translation puts them in (assets/<kind>/...) and their extension. */
const TYPES = [
  { id: "all", label: "All Assets", Icon: Folder },
  { id: "images", label: "Sprites & Textures", Icon: Image },
  { id: "models", label: "Models", Icon: Package },
  { id: "audio", label: "Audio", Icon: FileAudio },
  { id: "fonts", label: "Fonts", Icon: Type },
  { id: "other", label: "Other", Icon: Shapes },
] as const;
type AssetType = (typeof TYPES)[number]["id"];

const isImage = (asset: string) => /\.(png|jpe?g|webp|gif|bmp|svg)$/i.test(asset);
const isModel = (asset: string) => /\.(glb|gltf|obj)$/i.test(asset);
const isSound = (asset: string) => /\.(ogg|mp3|wav|m4a|aac|flac)$/i.test(asset);
const isFont = (asset: string) => /\.(ttf|otf|woff2?)$/i.test(asset);
const typeOf = (asset: string): AssetType => (isImage(asset) ? "images" : isModel(asset) ? "models" : isSound(asset) ? "audio" : isFont(asset) ? "fonts" : "other");
/** The badge on a tile: a tile sheet, a model, or the file's extension. */
const badgeOf = (asset: string) => (isModel(asset) ? "MODEL" : /tile/i.test(asset) && isImage(asset) ? "TILE" : (asset.split(".").pop() ?? "").toUpperCase());

/** The tabs along the top: the files, and the game's things by kind. */
const TABS: { id: "assets" | ThingKind; label: string }[] = [
  { id: "assets", label: "Assets" },
  { id: "character", label: "Characters" },
  { id: "item", label: "Items" },
  { id: "prop", label: "Props" },
  { id: "tool", label: "Tools" },
];

/** Tiles shown at once; the rest come with search or a narrower type. */
const LIMIT = 160;

interface Props {
  assets: string[];
  things: Thing[];
  readAsset: (asset: string) => Promise<ArrayBuffer>;
  /** Bumped when files change, so pictures are read again. */
  version: number;
  /** A scene is open (things can be added to it). */
  canAdd: boolean;
  onReplace: (asset: string) => void;
  /** Adds an image (as a sprite, `size` its pixels) or a model to the open scene. */
  onAdd: (asset: string, size?: [number, number]) => void;
  onPlace: (thing: Thing) => void;
  onOpenThing: (id: string) => void;
}

/** The project's files and things along the bottom: by type, searchable, with pictures. */
export default function AssetBrowser({ assets, things, readAsset, version, canAdd, onReplace, onAdd, onPlace, onOpenThing }: Props) {
  const [open, setOpen] = useState(true);
  const [tab, setTab] = useState<(typeof TABS)[number]["id"]>("assets");
  const [type, setType] = useState<AssetType>("all");
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();

  const counts = useMemo(() => {
    const c: Record<string, number> = { all: assets.length };
    for (const a of assets) c[typeOf(a)] = (c[typeOf(a)] ?? 0) + 1;
    return c;
  }, [assets]);
  const shownAssets = useMemo(() => assets.filter((a) => (type === "all" || typeOf(a) === type) && (!q || a.toLowerCase().includes(q))), [assets, type, q]);
  const shownThings = useMemo(() => things.filter((t) => t.kind === tab && (!q || t.name.toLowerCase().includes(q))), [things, tab, q]);
  const kindCount = (k: string) => things.filter((t) => t.kind === k).length;

  return (
    <section id="gx-assets" className={open ? "gx-assets open" : "gx-assets"}>
      <div className="gx-assets-head">
        <div className="gx-assets-tabs">
          {TABS.filter((t) => t.id === "assets" || kindCount(t.id) > 0).map((t) => (
            <button key={t.id} className={tab === t.id ? "active" : ""} onClick={() => setTab(t.id)}>
              {t.label}
            </button>
          ))}
        </div>
        <span className="status-spacer" />
        <button className="icon-button gx-mini" onClick={() => setOpen((o) => !o)} title={open ? "Hide the assets" : "Show the assets"}>
          {open ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
        </button>
      </div>
      {open && (
        <div className="gx-assets-body">
          <div className="gx-assets-side">
            <label className="gx-assets-search">
              <Search size={13} />
              <input placeholder={tab === "assets" ? "Search assets…" : "Search things…"} value={query} onChange={(e) => setQuery(e.target.value)} />
            </label>
            {tab === "assets" && (
              <ul className="gx-type-tree">
                {TYPES.filter((t) => t.id === "all" || counts[t.id]).map(({ id, label, Icon }) => (
                  <li key={id}>
                    <button className={type === id ? "active" : ""} onClick={() => setType(id)}>
                      <Icon size={13} />
                      <span className="grow">{label}</span>
                      <span className="count">{counts[id] ?? 0}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div className="gx-assets-grid">
            {tab === "assets" ? (
              <>
                {shownAssets.length === 0 && <p className="muted small">{assets.length ? "Nothing matches." : "No assets yet: translated games add their images, models and sounds here."}</p>}
                {shownAssets.slice(0, LIMIT).map((a) => (
                  <AssetTile key={a} asset={a} readAsset={readAsset} version={version} canAdd={canAdd} onReplace={onReplace} onAdd={onAdd} />
                ))}
                {shownAssets.length > LIMIT && <p className="muted small">{shownAssets.length - LIMIT} more: search or pick a type to narrow them down.</p>}
              </>
            ) : (
              <>
                {shownThings.length === 0 && <p className="muted small">Nothing matches.</p>}
                {shownThings.slice(0, LIMIT).map((t) => (
                  <ThingTile key={t.id} thing={t} readAsset={readAsset} version={version} canAdd={canAdd} onPlace={onPlace} onOpen={onOpenThing} />
                ))}
              </>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

/** An image asset's picture, read once its tile scrolls into view. */
function usePicture(asset: string | undefined, version: number, readAsset: (asset: string) => Promise<ArrayBuffer>) {
  const ref = useRef<HTMLDivElement>(null);
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!asset || !isImage(asset) || !ref.current) return;
    let made: string | null = null;
    let cancelled = false;
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((e) => e.isIntersecting)) return;
      observer.disconnect();
      readAsset(asset)
        .then((bytes) => {
          if (cancelled) return;
          made = URL.createObjectURL(new Blob([bytes], { type: asset.toLowerCase().endsWith(".svg") ? "image/svg+xml" : "" }));
          setUrl(made);
        })
        .catch(() => {});
    });
    observer.observe(ref.current);
    return () => {
      cancelled = true;
      observer.disconnect();
      if (made) URL.revokeObjectURL(made);
    };
  }, [asset, version, readAsset]);
  return { ref, url };
}

function AssetTile({ asset, readAsset, version, canAdd, onReplace, onAdd }: Pick<Props, "readAsset" | "version" | "canAdd" | "onReplace" | "onAdd"> & { asset: string }) {
  const { ref, url } = usePicture(asset, version, readAsset);
  const [size, setSize] = useState<[number, number] | undefined>();
  const [playing, setPlaying] = useState<HTMLAudioElement | null>(null);
  const name = asset.split("/").pop() ?? asset;

  useEffect(() => () => playing?.pause(), [playing]);

  const toggleSound = async () => {
    if (playing) {
      playing.pause();
      return setPlaying(null);
    }
    try {
      const bytes = await readAsset(asset);
      const audio = new Audio(URL.createObjectURL(new Blob([bytes])));
      audio.onended = () => setPlaying(null);
      await audio.play();
      setPlaying(audio);
    } catch {
      setPlaying(null);
    }
  };

  return (
    <div ref={ref} className="gx-asset-tile" title={asset}>
      <div className="gx-asset-thumb">
        {url ? (
          <img src={url} alt="" draggable={false} onLoad={(e) => setSize([e.currentTarget.naturalWidth, e.currentTarget.naturalHeight])} />
        ) : isSound(asset) ? (
          <FileAudio size={22} className="muted" />
        ) : isModel(asset) ? (
          <Package size={22} className="muted" />
        ) : null}
        <span className="gx-badge">{badgeOf(asset)}</span>
      </div>
      <div className="gx-asset-name tiny">{name}</div>
      <div className="gx-asset-actions">
        {size && <span className="muted tiny grow">{size[0]}×{size[1]}</span>}
        {isSound(asset) && (
          <button className="icon-button gx-mini" onClick={toggleSound} title={playing ? "Stop" : "Play"}>
            {playing ? <Square size={11} /> : <Play size={11} />}
          </button>
        )}
        {(isImage(asset) || isModel(asset)) && (
          <button className="icon-button gx-mini" onClick={() => onAdd(asset, size)} disabled={!canAdd} title={canAdd ? "Add to the scene" : "Open a scene to add it"}>
            <Plus size={11} />
          </button>
        )}
        <button className="icon-button gx-mini" onClick={() => onReplace(asset)} title="Replace this file (same kind)">
          <Replace size={11} />
        </button>
      </div>
    </div>
  );
}

function ThingTile({ thing, readAsset, version, canAdd, onPlace, onOpen }: Pick<Props, "readAsset" | "version" | "canAdd"> & { thing: Thing; onPlace: (t: Thing) => void; onOpen: (id: string) => void }) {
  const look = thing.parts?.find((p) => p.type === "sprite" || p.type === "model");
  const asset = look && "asset" in look ? look.asset : undefined;
  const { ref, url } = usePicture(asset, version, readAsset);
  return (
    <div ref={ref} className="gx-asset-tile gx-thing-tile" title={thing.description ?? thing.name} onClick={() => onOpen(thing.id)}>
      <div className="gx-asset-thumb">
        {url ? <img src={url} alt="" draggable={false} /> : <Shapes size={22} className="muted" />}
        {thing.role && <span className="gx-badge">{thing.role.toUpperCase()}</span>}
      </div>
      <div className="gx-asset-name tiny">{thing.name}</div>
      <div className="gx-asset-actions">
        <button
          className="icon-button gx-mini"
          onClick={(e) => {
            e.stopPropagation();
            onPlace(thing);
          }}
          disabled={!canAdd}
          title={canAdd ? "Place in the scene" : "Open a scene to place it"}
        >
          <Plus size={11} />
        </button>
      </div>
    </div>
  );
}
