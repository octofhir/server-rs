import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { tokens } from "../theme";

/** Octo design tokens (typography, spacing, radius, shadows, motion, layout). */
export function useDesignTokens() {
    return tokens;
}

export interface DisclosureState {
    isOpen: boolean;
    open: () => void;
    close: () => void;
    toggle: () => void;
}

/** Boolean state machine for modals/dropdowns/drawers. */
export function useDisclosureState(initialState = false): DisclosureState {
    const [isOpen, setIsOpen] = useState(initialState);
    const open = useCallback(() => setIsOpen(true), []);
    const close = useCallback(() => setIsOpen(false), []);
    const toggle = useCallback(() => setIsOpen((prev) => !prev), []);
    return useMemo(() => ({ isOpen, open, close, toggle }), [isOpen, open, close, toggle]);
}

export interface PersistentStateOptions<T> {
    key: string;
    defaultValue: T;
}

/** Persisted state synced to localStorage. */
export function usePersistentState<T>({ key, defaultValue }: PersistentStateOptions<T>) {
    const [value, setValue] = useState<T>(() => {
        if (typeof window === "undefined") return defaultValue;
        try {
            const item = localStorage.getItem(key);
            return item ? (JSON.parse(item) as T) : defaultValue;
        } catch {
            return defaultValue;
        }
    });
    const setItem = useCallback(
        (val: T | ((prev: T) => T)) => {
            setValue((prev) => {
                const next = val instanceof Function ? val(prev) : val;
                try {
                    localStorage.setItem(key, JSON.stringify(next));
                } catch {
                    /* quota / disabled */
                }
                return next;
            });
        },
        [key],
    );
    return [value, setItem] as const;
}

export function useMediaQuery(query: string, initialValue = false) {
    const [matches, setMatches] = useState(initialValue);
    useEffect(() => {
        const media = window.matchMedia(query);
        setMatches(media.matches);
        const listener = () => setMatches(media.matches);
        media.addEventListener("change", listener);
        return () => media.removeEventListener("change", listener);
    }, [query]);
    return matches;
}

export interface UseClipboardResult {
    copy: (text: string) => void;
    copied: boolean;
}

export function useClipboard(): UseClipboardResult {
    const [copied, setCopied] = useState(false);
    const copy = useCallback((text: string) => {
        navigator.clipboard.writeText(text).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        });
    }, []);
    return { copy, copied };
}

/** Returns a value debounced by `delay` milliseconds. */
export function useDebouncedValue<T>(value: T, delay: number): T {
    const [debounced, setDebounced] = useState(value);
    useEffect(() => {
        const handle = setTimeout(() => setDebounced(value), delay);
        return () => clearTimeout(handle);
    }, [value, delay]);
    return debounced;
}

/** Attach a ref to an element to invoke `handler` when a click lands outside it. */
export function useOutsideClick<T extends HTMLElement = HTMLElement>(handler: () => void) {
    const ref = useRef<T>(null);
    useEffect(() => {
        const listener = (event: MouseEvent | TouchEvent) => {
            if (!ref.current || ref.current.contains(event.target as Node)) return;
            handler();
        };
        document.addEventListener("mousedown", listener);
        document.addEventListener("touchstart", listener);
        return () => {
            document.removeEventListener("mousedown", listener);
            document.removeEventListener("touchstart", listener);
        };
    }, [handler]);
    return ref;
}

export interface ViewportSize {
    width: number;
    height: number;
}

export function useViewportSize(): ViewportSize {
    const [size, setSize] = useState<ViewportSize>(() => ({
        width: typeof window !== "undefined" ? window.innerWidth : 0,
        height: typeof window !== "undefined" ? window.innerHeight : 0,
    }));
    useEffect(() => {
        const handler = () => setSize({ width: window.innerWidth, height: window.innerHeight });
        window.addEventListener("resize", handler);
        return () => window.removeEventListener("resize", handler);
    }, []);
    return size;
}

/** Listens for keyboard shortcuts. Format: `["mod+k", () => …]`. */
export function useHotkeys(hotkeys: ReadonlyArray<readonly [string, (e: KeyboardEvent) => void]>) {
    useEffect(() => {
        const handler = (e: KeyboardEvent) => {
            const isMod = e.metaKey || e.ctrlKey;
            for (const [combo, cb] of hotkeys) {
                const parts = combo.toLowerCase().split("+").map((s) => s.trim());
                const wantsMod = parts.includes("mod") || parts.includes("ctrl") || parts.includes("cmd");
                const key = parts[parts.length - 1];
                if (e.key.toLowerCase() === key && (!wantsMod || isMod)) cb(e);
            }
        };
        document.addEventListener("keydown", handler);
        return () => document.removeEventListener("keydown", handler);
    }, [hotkeys]);
}

// =====================================================================
// === DEPRECATED legacy hooks — to be removed in a follow-up sweep ===
// =====================================================================

/** @deprecated use {@link useDisclosureState} */
export function useDisclosure(initialState = false) {
    const state = useDisclosureState(initialState);
    return [state.isOpen, { open: state.open, close: state.close, toggle: state.toggle }] as const;
}

/** @deprecated use {@link usePersistentState} */
export const useLocalStorage = usePersistentState;

/** @deprecated use {@link useOutsideClick} */
export const useClickOutside = useOutsideClick;

/** @deprecated implement inline */
export function useToggle<T = boolean>(options: readonly T[] = [false as T, true as T]) {
    const [value, setValue] = useState(options[0]);
    const toggle = useCallback(
        (val?: React.SetStateAction<T>) => {
            if (typeof val !== "undefined") setValue(val);
            else setValue((current) => (current === options[0] ? options[1] : options[0]));
        },
        [options],
    );
    return [value, toggle] as const;
}

/** @deprecated implement inline */
export function useInputState<T>(initialState: T) {
    const [value, setValue] = useState(initialState);
    const handleChange = useCallback((val: T | React.ChangeEvent<HTMLInputElement>) => {
        if (val && typeof val === "object" && "target" in val) {
            setValue((val as React.ChangeEvent<HTMLInputElement>).target.value as T);
        } else {
            setValue(val as T);
        }
    }, []);
    return [value, handleChange] as const;
}

/** @deprecated unused */
export function useFocusTrap() {
    return useRef(null);
}
/** @deprecated unused */
export function useScrollIntoView() {
    return { scrollIntoView: () => {}, targetRef: useRef(null), scrollableRef: useRef(null) };
}
/** @deprecated unused */
export function useElementSize() {
    return { ref: useRef(null), width: 0, height: 0 };
}
