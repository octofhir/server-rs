import { useLayoutEffect, useMemo, type ReactNode } from "react";
import { ThemeProvider, ToasterComponent, ToasterProvider } from "@gravity-ui/uikit";
import { ConfirmModalHost } from "#/shared/lib/confirm-modal";
import { toaster } from "#/shared/lib/toaster";
import {
    ColorSchemeProvider,
    tokens,
    useColorScheme,
    type ColorScheme,
    type ColorSchemePreference,
} from "#/shared/theme";
import { generateCSSVariables } from "#/shared/theme/utils";

// Stylesheet imports live in `src/styles.ts` (re-exported via the package
// barrel and the `@octofhir/ui-kit/styles` entry point). Importing here would
// duplicate them in the bundle.

export interface UIProviderProps {
    children: ReactNode;
    /** Initial color-scheme preference if none has been persisted yet. */
    defaultColorScheme?: ColorSchemePreference;
}

export function UIProvider({ children, defaultColorScheme = "light" }: UIProviderProps) {
    return (
        <ColorSchemeProvider defaultColorScheme={defaultColorScheme}>
            <UIProviderInner>{children}</UIProviderInner>
        </ColorSchemeProvider>
    );
}

const ROOT_CLASS = "g-root";
const themeClass = (scheme: ColorScheme) => `g-root_theme_${scheme}`;

function applyBodyClasses(scheme: ColorScheme) {
    const body = document.body;
    if (!body) return () => {};
    body.classList.add(ROOT_CLASS);
    // Strip any stale `g-root_theme_*` first.
    [...body.classList].forEach((cls) => {
        if (cls.startsWith("g-root_theme_") && cls !== themeClass(scheme)) {
            body.classList.remove(cls);
        }
    });
    body.classList.add(themeClass(scheme));
    document.documentElement.dataset.theme = scheme;
    return () => {
        // No-op cleanup: leaving `g-root` on body is intentional — Gravity's
        // tokens stay in scope even if the provider is briefly unmounted.
    };
}

export function UIProviderInner({ children }: { children: ReactNode }) {
    const { colorScheme, preference } = useColorScheme();

    const cssVars = useMemo(() => {
        const { scheme, ...rest } = tokens;
        const globalVars = generateCSSVariables(rest);
        const schemeVars = generateCSSVariables(scheme[colorScheme]);
        return { ...globalVars, ...schemeVars };
    }, [colorScheme]);

    // Belt + suspenders: explicitly add `g-root g-root_theme_<scheme>` to
    // <body> and `data-theme` to <html>. Gravity's `<ThemeProvider>` does this
    // too, but we want the classes present even before its effect runs and we
    // want `data-theme` for legacy CSS that pre-dated the migration.
    useLayoutEffect(() => applyBodyClasses(colorScheme), [colorScheme]);

    // Push our `--octo-*` design tokens onto <html> so Gravity's portaled
    // components (Toaster, Modal, Drawer, Popup) — appended to document.body —
    // can resolve them too.
    useLayoutEffect(() => {
        const target = document.documentElement;
        const previous = new Map<string, string>();
        for (const [k, v] of Object.entries(cssVars)) {
            previous.set(k, target.style.getPropertyValue(k));
            target.style.setProperty(k, v);
        }
        return () => {
            for (const [k, prev] of previous) {
                if (prev) target.style.setProperty(k, prev);
                else target.style.removeProperty(k);
            }
        };
    }, [cssVars]);

    return (
        <ThemeProvider theme={preference === "auto" ? "system" : preference}>
            <ToasterProvider toaster={toaster}>
                {children}
                <ToasterComponent />
                <ConfirmModalHost />
            </ToasterProvider>
        </ThemeProvider>
    );
}
