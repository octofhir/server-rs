import {
    MantineProvider,
    ColorSchemeScript,
    type MantineProviderProps,
    Badge,
    Button,
    TextInput,
    SegmentedControl
} from "@mantine/core";
import { ModalsProvider } from "@mantine/modals";
import { Notifications } from "@mantine/notifications";
import { theme as baseTheme, resolver } from "#/shared/theme";
import type { ReactNode } from "react";

// Import CSS modules via deep import (FSD exception for assets if needed, or but here we use relative or alias)
import badgeClasses from "#/shared/ui/Badge/Badge.module.css";
import buttonClasses from "#/shared/ui/Button/Button.module.css";
import textInputClasses from "#/shared/ui/TextInput/TextInput.module.css";
import segmentedControlClasses from "#/shared/ui/SegmentedControl/SegmentedControl.module.css";

// Import Mantine styles
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";

export interface UIProviderProps extends Omit<MantineProviderProps, "theme" | "cssVariablesResolver"> {
    children: ReactNode;
}

const theme = {
    ...baseTheme,
    components: {
        Badge: Badge.extend({
            classNames: badgeClasses,
            defaultProps: {
                variant: "light",
                radius: "sm",
            },
        }),
        Button: Button.extend({
            classNames: buttonClasses,
            defaultProps: {
                radius: "sm",
            },
        }),
        TextInput: TextInput.extend({
            classNames: textInputClasses,
            defaultProps: {
                radius: "sm",
            },
        }),
        SegmentedControl: SegmentedControl.extend({
            classNames: segmentedControlClasses,
            defaultProps: {
                radius: "sm",
            },
        }),
    },
};

/**
 * Global UI Provider for OctoFHIR applications.
 * Automatically configures Mantine with OctoFHIR theme and CSS variables resolver.
 */
export function UIProvider({ children, ...props }: UIProviderProps) {
    return (
        <>
            <ColorSchemeScript defaultColorScheme="auto" />
            <MantineProvider
                theme={theme}
                cssVariablesResolver={resolver}
                defaultColorScheme="auto"
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
