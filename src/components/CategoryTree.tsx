import { ChevronDown, ChevronRight, Search } from "lucide-react";
import { useMemo, useState, type RefObject } from "react";
import { categories } from "../lib/categories";
import type { Category, GameObject } from "../lib/types";
import { CategoryIcon, ObjectIcon } from "../lib/visuals";

const PAGE = 200;

const ROLE_GROUPS: Record<string, string> = {
  Player: "players",
  Enemy: "enemies",
  NPC: "NPCs",
  Ally: "allies",
};

function roleGroup(obj: GameObject): string {
  return obj.role ? (ROLE_GROUPS[obj.role] ?? obj.role) : "other characters";
}

/** A placement in a game category made on weak evidence. */
function isUnsure(obj: GameObject): boolean {
  return obj.confidence < 0.5 && obj.category !== "code" && obj.category !== "unsorted";
}

interface Props {
  objects: GameObject[];
  /** Files the user has changed; objects containing one are marked. */
  edited: Set<string>;
  selectedId: string | null;
  onSelect: (id: string) => void;
  /** Lets the header's search button focus the search box. */
  searchRef?: RefObject<HTMLInputElement | null>;
}

/** Category -> kind -> objects. Kinds are shown only when a category has more than one. */
export default function CategoryTree({ objects, edited, selectedId, onSelect, searchRef }: Props) {
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const [shown, setShown] = useState<Record<string, number>>({});

  const query = filter.trim().toLowerCase();
  const groups = useMemo(() => {
    const byCategory = new Map<Category, Map<string, GameObject[]>>();
    for (const obj of objects) {
      if (query && !obj.displayName.toLowerCase().includes(query) && !obj.kind.includes(query)) continue;
      let kinds = byCategory.get(obj.category);
      if (!kinds) byCategory.set(obj.category, (kinds = new Map()));
      // Characters are grouped by role (Player, Enemy...), others by kind.
      const group = obj.category === "characters" ? roleGroup(obj) : obj.kind;
      const list = kinds.get(group) ?? [];
      list.push(obj);
      kinds.set(group, list);
    }
    for (const kinds of byCategory.values()) {
      for (const list of kinds.values()) {
        list.sort((a, b) => a.displayName.localeCompare(b.displayName));
      }
    }
    return byCategory;
  }, [objects, query]);

  // While searching, everything that matches is expanded.
  const isOpen = (key: string) => (query ? true : open.has(key));
  const toggle = (key: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const chevron = (expanded: boolean) =>
    expanded ? <ChevronDown size={14} className="chevron" /> : <ChevronRight size={14} className="chevron" />;

  const renderObjects = (key: string, list: GameObject[]) => {
    const limit = shown[key] ?? PAGE;
    return (
      <ul className="tree-objects">
        {list.slice(0, limit).map((obj) => (
          <li key={obj.id}>
            <button
              className={obj.id === selectedId ? "tree-row object selected" : "tree-row object"}
              onClick={() => onSelect(obj.id)}
              title={obj.displayName}
            >
              <ObjectIcon object={obj} />
              <span className="tree-name">{obj.displayName}</span>
              {obj.sourceFiles.some((f) => edited.has(f)) && <span className="tag">edited</span>}
              {obj.category !== obj.autoCategory && <span className="tag">moved</span>}
              {obj.category === obj.autoCategory && isUnsure(obj) && (
                <span className="tag unsure-tag" title={`Only ${Math.round(obj.confidence * 100)}% sure: ${obj.reason}`}>
                  unsure
                </span>
              )}
            </button>
          </li>
        ))}
        {list.length > limit && (
          <li>
            <button
              className="tree-row more"
              onClick={() => setShown((s) => ({ ...s, [key]: limit + PAGE }))}
            >
              Show {Math.min(PAGE, list.length - limit)} more of {list.length - limit}
            </button>
          </li>
        )}
      </ul>
    );
  };

  return (
    <div className="tree">
      <label className="search-box">
        <Search size={15} />
        <input
          ref={searchRef}
          type="text"
          placeholder="Search by name or kind…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setFilter("")}
        />
      </label>
      <ul className="tree-categories">
        {categories.map((cat) => {
          const kinds = groups.get(cat.id);
          const count = kinds ? [...kinds.values()].reduce((n, l) => n + l.length, 0) : 0;
          if (query && count === 0) return null;
          const expanded = count > 0 && isOpen(cat.id);
          const kindList = kinds ? [...kinds.entries()].sort(([a], [b]) => a.localeCompare(b)) : [];
          return (
            <li key={cat.id}>
              <button
                className="tree-row category"
                onClick={() => toggle(cat.id)}
                disabled={count === 0}
                title={cat.hint}
              >
                {chevron(expanded)}
                <CategoryIcon category={cat.id} />
                <span className="tree-name">{cat.label}</span>
                <span className="count">{count}</span>
              </button>
              {expanded &&
                (kindList.length === 1 ? (
                  renderObjects(cat.id, kindList[0][1])
                ) : (
                  <ul className="tree-kinds">
                    {kindList.map(([kind, list]) => {
                      const key = `${cat.id}/${kind}`;
                      return (
                        <li key={kind}>
                          <button className="tree-row kind" onClick={() => toggle(key)}>
                            {chevron(isOpen(key))}
                            <span className="tree-name">{kind}</span>
                            <span className="count">{list.length}</span>
                          </button>
                          {isOpen(key) && renderObjects(key, list)}
                        </li>
                      );
                    })}
                  </ul>
                ))}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
