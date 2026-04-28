import { useMemo, type ReactNode } from "react";
import { ThemeProvider, ToasterComponent, ToasterProvider } from "@gravity-ui/uikit";
import { ConfirmModalHost } from "#/shared/lib/confirm-modal";
import { toaster } from "#/shared/lib/toaster";
import {
    ColorSchemeProvider,
    tokens,
    useColorScheme,
    type ColorSchemePreference,
} from "#/shared/theme";
import { generateCSSVariables } from "#/shared/theme/utils";

import "@gravity-ui/uikit/styles/fonts.css";
import "@gravity-ui/uikit/styles/styles.css";
import "#/shared/theme/fonts.css";
import "#/shared/theme/gravity-overrides.css";

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

function UIProviderInner({ children }: { children: ReactNode }) {
    const { colorScheme, preference } = useColorScheme();

    const cssVars = useMemo(() => {
        const { scheme, ...rest } = tokens;
        const globalVars = generateCSSVariables(rest);
        const schemeVars = generateCSSVariables(scheme[colorScheme]);
        return { ...globalVars, ...schemeVars };
    }, [colorScheme]);

    return (
        <ThemeProvider theme={preference === "auto" ? "system" : preference}>
            <div
                className="octo-ui-provider g-root"
                style={{ display: "contents", ...cssVars } as React.CSSProperties}
            >
                <ToasterProvider toaster={toaster}>
                    {children}
                    <ToasterComponent />
                    <ConfirmModalHost />
                </ToasterProvider>
            </div>
        </ThemeProvider>
    );
}
