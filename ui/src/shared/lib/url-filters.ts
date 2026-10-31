import { useCallback, useMemo } from "react";
import { useSearchParams } from "react-router-dom";

type UrlValue = string | string[] | undefined;

const NONE: readonly never[] = [];

interface UrlFilterKeys<T> {
  /** Single-value keys, kept as `?key=value`. */
  text?: readonly (keyof T & string)[];
  /** Multi-value keys, kept as `?key=a,b`. */
  lists?: readonly (keyof T & string)[];
}

/**
 * Page filters that live in the URL, so a filtered view survives reloads, Back and sharing.
 * Only the listed keys are read; empty values are removed from the URL. Updates replace the
 * history entry instead of adding one per keystroke. Pass `keys` as a module-level constant.
 */
export function useUrlFilters<T extends Record<string, UrlValue>>(
  keys: UrlFilterKeys<T>
): [Partial<T>, (patch: Partial<T>) => void, () => void] {
  const [params, setParams] = useSearchParams();
  const text = keys.text ?? NONE;
  const lists = keys.lists ?? NONE;
  const filters = useMemo(() => {
    const out: Record<string, UrlValue> = {};
    for (const key of text) {
      const value = params.get(key);
      if (value) out[key] = value;
    }
    for (const key of lists) {
      const value = params.get(key);
      if (value) out[key] = value.split(",").filter(Boolean);
    }
    return out as Partial<T>;
  }, [params, text, lists]);

  const update = useCallback(
    (patch: Partial<T>) =>
      setParams(
        (previous) => {
          const next = new URLSearchParams(previous);
          for (const [key, value] of Object.entries(patch)) {
            const serialized = Array.isArray(value) ? value.join(",") : (value ?? "");
            if (serialized) next.set(key, serialized);
            else next.delete(key);
          }
          return next;
        },
        { replace: true }
      ),
    [setParams]
  );

  const clear = useCallback(
    () =>
      setParams(
        (previous) => {
          const next = new URLSearchParams(previous);
          for (const key of [...text, ...lists]) next.delete(key);
          return next;
        },
        { replace: true }
      ),
    [setParams, text, lists]
  );

  return [filters, update, clear];
}
