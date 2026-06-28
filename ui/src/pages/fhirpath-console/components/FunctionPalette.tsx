import { Search } from "lucide-react";
import { useMemo, useState } from "react";
import classes from "../FhirPathConsolePage.module.css";
import { FHIRPATH_FUNCTIONS, type FhirPathFn } from "../functions";

interface Props {
  onInsert: (fn: FhirPathFn) => void;
}

export function FunctionPalette({ onInsert }: Props) {
  const [query, setQuery] = useState("");

  const categories = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return FHIRPATH_FUNCTIONS;
    return FHIRPATH_FUNCTIONS.map((cat) => ({
      ...cat,
      functions: cat.functions.filter(
        (fn) => fn.name.toLowerCase().includes(q) || fn.doc.toLowerCase().includes(q)
      ),
    })).filter((cat) => cat.functions.length > 0);
  }, [query]);

  return (
    <div className={classes.palette}>
      <div className={classes.paletteSearch}>
        <Search size={13} />
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search functions…"
          className={classes.paletteInput}
        />
      </div>
      <div className={classes.paletteScroll}>
        {categories.length === 0 ? (
          <div className={classes.paletteEmpty}>No functions match “{query}”.</div>
        ) : (
          categories.map((cat) => (
            <div key={cat.label} className={classes.paletteGroup}>
              <div className={classes.paletteGroupLabel}>{cat.label}</div>
              <div className={classes.paletteChips}>
                {cat.functions.map((fn) => (
                  <button
                    key={fn.name}
                    type="button"
                    className={classes.paletteFn}
                    title={`${fn.signature} — ${fn.doc}`}
                    onClick={() => onInsert(fn)}
                  >
                    {fn.name}
                  </button>
                ))}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
