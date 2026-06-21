import { useLayoutEffect, useMemo, type ReactNode } from "react";
import { ConfirmModalHost } from "#/shared/lib/confirm-modal";
import { ToasterHost } from "#/shared/lib/ToasterHost";
import {
    ColorSchemeProvider,
    createOctoTheme,
    getThemeCssVariables,
    useColorScheme,
    type ColorScheme,
    type ColorSchemePreference,
    type OctoThemeInput,
} from "#/shared/theme";
import { generateCSSVariables } from "#/shared/theme/utils";

// Stylesheet imports live in `src/styles.ts` (re-exported via the package
// barrel and the `@octofhir/ui-kit/styles` entry point). Importing here would
// duplicate them in the bundle.

export interface UIProviderProps {
    children: ReactNode;
    /** Initial color-scheme preference if none has been persisted yet. */
    defaultColorScheme?: ColorSchemePreference;
    /**
     * Theme override merged over the built-in OctoFHIR design tokens.
     * Pass `{tokens: ...}` for structured overrides and `cssVariables` for
     * explicit custom CSS vars.
     */
    theme?: OctoThemeInput;
}

export function UIProvider({ children, defaultColorScheme = "light", theme }: UIProviderProps) {
    return (
        <ColorSchemeProvider defaultColorScheme={defaultColorScheme}>
            <UIProviderInner theme={theme}>{children}</UIProviderInner>
        </ColorSchemeProvider>
    );
}

const ROOT_CLASS = "octo-ui-provider";

function applyBodyClasses(scheme: ColorScheme) {
    const body = document.body;
    if (!body) return () => {};
    body.classList.add(ROOT_CLASS);
    document.documentElement.dataset.theme = scheme;
    return () => {};
}

export function UIProviderInner({
    children,
    theme,
}: {
    children: ReactNode;
    theme?: OctoThemeInput;
}) {
    const { colorScheme } = useColorScheme();

    const cssVars = useMemo(() => {
        const resolvedTheme = createOctoTheme(theme);
        const { scheme, ...rest } = resolvedTheme;
        const globalVars = generateCSSVariables(rest);
        const schemeVars = generateCSSVariables(scheme[colorScheme]);
        const explicitVars = getThemeCssVariables(theme);
        return { ...globalVars, ...schemeVars, ...explicitVars };
    }, [colorScheme, theme]);

    // Mark <body> and set `data-theme` on <html> for theme-scoped CSS.
    useLayoutEffect(() => applyBodyClasses(colorScheme), [colorScheme]);

    // Push the `--octo-*` design tokens onto <html> so portaled components
    // (Toaster, Modal, Drawer, Popup) — appended to document.body — resolve them.
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
        <>
            {children}
            <ToasterHost />
            <ConfirmModalHost />
        </>
    );
}
