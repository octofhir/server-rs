import {
    MantineProvider,
    ColorSchemeScript,
    mergeThemeOverrides,
    type MantineProviderProps,
    type MantineThemeOverride,
} from "@mantine/core";
import { ModalsProvider } from "@mantine/modals";
import { Notifications } from "@mantine/notifications";
import { theme as baseTheme, resolver } from "#/shared/theme";
import { useMemo, type ReactNode } from "react";

import "@mantine/core/styles.layer.css";
import "@mantine/dates/styles.layer.css";
import "@mantine/notifications/styles.layer.css";
import "#/shared/theme/fonts.css";

export interface UIProviderProps extends Omit<MantineProviderProps, "theme" | "cssVariablesResolver"> {
    children: ReactNode;
    defaultColorScheme?: "light" | "dark" | "auto";
    /** Optional theme overrides — deep-merged with the base OctoFHIR theme */
    theme?: MantineThemeOverride;
}

export function UIProvider({ children, defaultColorScheme = "light", theme: userTheme, ...props }: UIProviderProps) {
    const mergedTheme = useMemo(
        () => (userTheme ? mergeThemeOverrides(baseTheme, userTheme) : baseTheme),
        [userTheme],
    );

    return (
        <>
            <ColorSchemeScript defaultColorScheme={defaultColorScheme} />
            <MantineProvider
                theme={mergedTheme}
                cssVariablesResolver={resolver}
                defaultColorScheme={defaultColorScheme}
                {...props}
            >
                <Notifications position="top-right" />
                <ModalsProvider>
                    {children}
                </ModalsProvider>
            </MantineProvider>
        </>
    );
}
