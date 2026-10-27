import { useMemo, useState } from "react";
import { categories } from "../lib/categories";
import type { Category, GameObject } from "../lib/types";

const PAGE = 200;

interface Props {
  objects: GameObject[];
  /** Files the user has changed; objects containing one are marked. */
  edited: Set<string>;
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/** Category -> kind -> objects. Kinds are shown only when a category has more than one. */
export default function CategoryTree({ objects, edited, selectedId, onSelect }: Props) {
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
      const list = kinds.get(obj.kind) ?? [];
      list.push(obj);
      kinds.set(obj.kind, list);
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
              <span className="tree-name">{obj.displayName}</span>
              {obj.sourceFiles.some((f) => edited.has(f)) && <span className="tag">edited</span>}
              {obj.category !== obj.autoCategory && <span className="tag">moved</span>}
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
      <input
        type="text"
        className="tree-search"
        placeholder="Search by name or kind"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
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
                <span className={expanded ? "chevron open" : "chevron"}>›</span>
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
                            <span className={isOpen(key) ? "chevron open" : "chevron"}>›</span>
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
