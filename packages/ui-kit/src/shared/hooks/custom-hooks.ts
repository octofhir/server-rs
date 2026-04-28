import { useCallback, useEffect, useRef, useState } from "react";
import { tokens } from "../theme";

// Note: `useColorScheme` lives in `shared/theme/color-scheme.tsx` and is the
// canonical source — re-exported via `shared/theme/index.ts`. Importing it
// here would cause a duplicate star-export from the package root.

/** Octo design tokens (typography, spacing, radius, shadows, motion, layout). */
export function useDesignTokens() {
    return tokens;
}

export function useDisclosure(initialState = false) {
    const [isOpen, setIsOpen] = useState(initialState);
    const open = useCallback(() => setIsOpen(true), []);
    const close = useCallback(() => setIsOpen(false), []);
    const toggle = useCallback(() => setIsOpen((prev) => !prev), []);
    return [isOpen, { open, close, toggle }] as const;
}

export function useToggle<T = boolean>(options: readonly T[] = [false as T, true as T]) {
    const [value, setValue] = useState(options[0]);
    const toggle = useCallback((val?: React.SetStateAction<T>) => {
        if (typeof val !== 'undefined') setValue(val);
        else setValue((current) => current === options[0] ? options[1] : options[0]);
    }, [options]);
    return [value, toggle] as const;
}

export function useLocalStorage<T>({ key, defaultValue }: { key: string, defaultValue: T }) {
    const [value, setValue] = useState<T>(() => {
        if (typeof window === "undefined") return defaultValue;
        try { const item = localStorage.getItem(key); return item ? JSON.parse(item) : defaultValue; } 
        catch { return defaultValue; }
    });
    const setItem = useCallback((val: T | ((prev: T) => T)) => {
        setValue((prev) => {
            const next = val instanceof Function ? val(prev) : val;
            localStorage.setItem(key, JSON.stringify(next));
            return next;
        });
    }, [key]);
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

export function useClipboard() {
    const [copied, setCopied] = useState(false);
    const copy = useCallback((text: string) => {
        navigator.clipboard.writeText(text).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 2000);
        });
    }, []);
    return { copy, copied };
}

export function useDebouncedValue<T>(value: T, delay: number) {
    const [debouncedValue, setDebouncedValue] = useState(value);
    useEffect(() => {
        const handler = setTimeout(() => setDebouncedValue(value), delay);
        return () => clearTimeout(handler);
    }, [value, delay]);
    return [debouncedValue];
}

export function useClickOutside<T extends HTMLElement = any>(handler: () => void) {
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

export function useViewportSize() {
    const [windowSize, setWindowSize] = useState({ 
        width: typeof window !== "undefined" ? window.innerWidth : 0, 
        height: typeof window !== "undefined" ? window.innerHeight : 0 
    });
    useEffect(() => {
        const handleResize = () => setWindowSize({ width: window.innerWidth, height: window.innerHeight });
        window.addEventListener("resize", handleResize);
        return () => window.removeEventListener("resize", handleResize);
    }, []);
    return windowSize;
}

export function useInputState<T>(initialState: T) {
    const [value, setValue] = useState(initialState);
    const handleChange = useCallback((val: T | React.ChangeEvent<any>) => {
        if (val && typeof val === 'object' && 'target' in val && 'value' in val.target) {
            setValue(val.target.value);
        } else {
            setValue(val as T);
        }
    }, []);
    return [value, handleChange] as const;
}

export function useHotkeys(hotkeys: [string, (e: KeyboardEvent) => void][]) {
    useEffect(() => {
        const handler = (e: KeyboardEvent) => {
            for (const [key, cb] of hotkeys) {
                if (e.key.toLowerCase() === key.toLowerCase()) cb(e);
            }
        };
        document.addEventListener("keydown", handler);
        return () => document.removeEventListener("keydown", handler);
    }, [hotkeys]);
}

export function useFocusTrap() { return useRef(null); }
export function useScrollIntoView() { return { scrollIntoView: () => {}, targetRef: useRef(null), scrollableRef: useRef(null) }; }
export function useElementSize() { return { ref: useRef(null), width: 0, height: 0 }; }
export function useCombobox() { return { store: {} }; }
