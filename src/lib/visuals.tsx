import {
  AppWindow,
  AudioLines,
  CircleQuestionMark,
  CodeXml,
  Cpu,
  File,
  FileCode,
  FileImage,
  FileMusic,
  FileText,
  Image,
  Map,
  Package,
  Users,
  type LucideIcon,
} from "lucide-react";
import type { Category, ContainerKind, GameObject } from "./types";

/** Icon and color for each category, used in the tree and the inspector. */
export const categoryVisual: Record<Category, { Icon: LucideIcon; color: string }> = {
  characters: { Icon: Users, color: "#6c8cff" },
  core: { Icon: Cpu, color: "#38bdf8" },
  map: { Icon: Map, color: "#34d399" },
  items: { Icon: Package, color: "#f59e0b" },
  assets: { Icon: Image, color: "#22d3ee" },
  audio: { Icon: AudioLines, color: "#f472b6" },
  ui: { Icon: AppWindow, color: "#a78bfa" },
  code: { Icon: CodeXml, color: "#60a5fa" },
  unsorted: { Icon: CircleQuestionMark, color: "#94a3b8" },
};

export function CategoryIcon({ category, size = 15 }: { category: Category; size?: number }) {
  const { Icon, color } = categoryVisual[category];
  return (
    <span className="category-icon" style={{ color, background: `${color}22` }}>
      <Icon size={size} strokeWidth={2.2} />
    </span>
  );
}

const IMAGE = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "svg", "ico"];
/** Godot's exported textures; GameX previews the image inside. */
const TEXTURE = ["ctex"];
const AUDIO = ["ogg", "mp3", "wav", "m4a", "aac", "flac", "mid", "midi"];
const CODE = ["java", "kt", "smali", "lua", "js", "ts", "py", "gd", "cs", "c", "cpp", "h", "glsl", "shader"];

/** A short type label and icon for a file path. */
export function fileType(path: string): { label: string; Icon: LucideIcon } {
  const ext = (path.split(".").pop() ?? "").toLowerCase();
  if (IMAGE.includes(ext)) return { label: "Image", Icon: FileImage };
  if (TEXTURE.includes(ext)) return { label: "Texture", Icon: FileImage };
  if (["oggvorbisstr", "sample", "mp3str"].includes(ext)) return { label: "Godot audio", Icon: FileMusic };
  if (ext === "gdc") return { label: "Compiled script", Icon: FileCode };
  if (ext === "scn") return { label: "Scene", Icon: File };
  if (AUDIO.includes(ext)) return { label: "Audio", Icon: FileMusic };
  if (ext === "java") return { label: "Java", Icon: FileCode };
  if (ext === "smali") return { label: "Smali", Icon: FileCode };
  if (CODE.includes(ext)) return { label: ext.toUpperCase(), Icon: FileCode };
  if (["xml", "json", "yml", "yaml", "txt", "csv", "ini", "cfg", "md"].includes(ext))
    return { label: ext.toUpperCase(), Icon: FileText };
  return { label: ext ? ext.toUpperCase() : "File", Icon: File };
}

/** The icon for an object in lists, from its first file. */
export function ObjectIcon({ object, size = 14 }: { object: GameObject; size?: number }) {
  const { Icon } = object.kind === "image set" ? { Icon: Image } : fileType(object.sourceFiles[0] ?? "");
  return <Icon size={size} className="object-icon" />;
}

export const containerLongLabel: Record<ContainerKind, string> = {
  apk: "Android APK",
  zip: "ZIP archive",
  folder: "Folder",
};

export function isImagePath(path: string) {
  const ext = (path.split(".").pop() ?? "").toLowerCase();
  return IMAGE.includes(ext) || TEXTURE.includes(ext);
}

/** Exported engine formats GameX can show but not edit yet. */
export const UNEDITABLE_FORMATS = new Set(["ctex", "scn", "gdc", "res", "oggvorbisstr", "sample", "mp3str", "fontdata"]);
