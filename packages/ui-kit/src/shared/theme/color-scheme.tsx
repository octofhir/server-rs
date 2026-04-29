import {
    createContext,
    useCallback,
    useContext,
    useEffect,
    useMemo,
    useState,
    type ReactNode,
} from "react";

export type ColorScheme = "light" | "dark";
export type ColorSchemePreference = ColorScheme | "auto";

export interface ColorSchemeContextValue {
    /** Effective scheme actually applied to the UI. */
    colorScheme: ColorScheme;
    /** Raw user preference (may be `auto`). */
    preference: ColorSchemePreference;
    setColorScheme: (next: ColorSchemePreference) => void;
    toggleColorScheme: () => void;
}

const STORAGE_KEY = "octofhir.colorScheme";

const ColorSchemeContext = createContext<ColorSchemeContextValue | undefined>(undefined);

function readStoredPreference(fallback: ColorSchemePreference): ColorSchemePreference {
    if (typeof window === "undefined") return fallback;
    try {
        const v = window.localStorage.getItem(STORAGE_KEY);
        if (v === "light" || v === "dark" || v === "auto") return v;
    } catch {
        /* ignore */
    }
    return fallback;
}

function resolveSystemScheme(): ColorScheme {
    if (typeof window === "undefined" || !window.matchMedia) return "light";
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export interface ColorSchemeProviderProps {
    children: ReactNode;
    defaultColorScheme?: ColorSchemePreference;
    /**
     * When provided, switches the provider to controlled mode. Useful for
     * Storybook globals or top-level apps that source the scheme from a router
     * / settings page. `setColorScheme` is forwarded to `onColorSchemeChange`.
     */
    colorScheme?: ColorSchemePreference;
    onColorSchemeChange?: (next: ColorSchemePreference) => void;
    /** Disable persistence to `localStorage`. Useful for storybook decorators. */
    persist?: boolean;
}

export function ColorSchemeProvider({
    children,
    defaultColorScheme = "light",
    colorScheme: controlledScheme,
    onColorSchemeChange,
    persist = true,
}: ColorSchemeProviderProps) {
    const isControlled = controlledScheme !== undefined;
    const [internalPreference, setInternalPreference] = useState<ColorSchemePreference>(() =>
        persist ? readStoredPreference(defaultColorScheme) : defaultColorScheme,
    );
    const preference = isControlled ? (controlledScheme as ColorSchemePreference) : internalPreference;
    const [systemScheme, setSystemScheme] = useState<ColorScheme>(() => resolveSystemScheme());

    useEffect(() => {
        if (typeof window === "undefined" || !window.matchMedia) return;
        const mql = window.matchMedia("(prefers-color-scheme: dark)");
        const handler = (e: MediaQueryListEvent) => setSystemScheme(e.matches ? "dark" : "light");
        mql.addEventListener("change", handler);
        return () => mql.removeEventListener("change", handler);
    }, []);

    const colorScheme: ColorScheme = preference === "auto" ? systemScheme : preference;

    const setColorScheme = useCallback(
        (next: ColorSchemePreference) => {
            if (!isControlled) setInternalPreference(next);
            onColorSchemeChange?.(next);
            if (persist && typeof window !== "undefined") {
                try {
                    window.localStorage.setItem(STORAGE_KEY, next);
                } catch {
                    /* ignore */
                }
            }
        },
        [isControlled, onColorSchemeChange, persist],
    );

    const toggleColorScheme = useCallback(() => {
        setColorScheme(colorScheme === "dark" ? "light" : "dark");
    }, [colorScheme, setColorScheme]);

    const value = useMemo<ColorSchemeContextValue>(
        () => ({ colorScheme, preference, setColorScheme, toggleColorScheme }),
        [colorScheme, preference, setColorScheme, toggleColorScheme],
    );

    return <ColorSchemeContext.Provider value={value}>{children}</ColorSchemeContext.Provider>;
}

export function useColorScheme(): ColorSchemeContextValue {
    const ctx = useContext(ColorSchemeContext);
    if (!ctx) {
        return {
            colorScheme: "light",
            preference: "light",
            setColorScheme: () => {},
            toggleColorScheme: () => {},
        };
    }
    return ctx;
}
