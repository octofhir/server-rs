import {
    createTheme,
    type MantineThemeOverride,
    type CSSVariablesResolver,
    Badge,
    Button,
    TextInput,
    SegmentedControl,
} from "@mantine/core";
import { palette } from "./colors";
import { tokens, type OctoTokens } from "./tokens";
import { generateCSSVariables } from "./utils";

import badgeClasses from "../ui/Badge/Badge.module.css";
import buttonClasses from "../ui/Button/Button.module.css";
import textInputClasses from "../ui/TextInput/TextInput.module.css";
import segmentedControlClasses from "../ui/SegmentedControl/SegmentedControl.module.css";

export type OctoThemeOther = OctoTokens;

declare module "@mantine/core" {
    export interface MantineThemeOther extends OctoThemeOther { }
}

export const theme: MantineThemeOverride = createTheme({
    primaryColor: "primary",
    primaryShade: { light: 5, dark: 4 },
    defaultGradient: { from: "primary.5", to: "fire.5", deg: 135 },
    colors: palette,
    fontFamily: tokens.typography.family,
    fontFamilyMonospace: tokens.typography.mono,
    headings: {
        fontFamily: tokens.typography.heading,
        fontWeight: tokens.typography.weight.semibold,
    },
    fontSizes: tokens.typography.size,
    spacing: {
        xs: tokens.spacing.xs,
        sm: tokens.spacing.sm,
        md: tokens.spacing.md,
        lg: tokens.spacing.lg,
        xl: tokens.spacing.xl,
    },
    radius: {
        xs: tokens.radius.xs,
        sm: tokens.radius.sm,
        md: tokens.radius.md,
        lg: tokens.radius.lg,
        xl: tokens.radius.xl,
    },
    shadows: tokens.shadow,
    other: {
        ...tokens,
    },
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
                radius: "md",
                size: "sm",
            },
        }),
        TextInput: TextInput.extend({
            classNames: textInputClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        SegmentedControl: SegmentedControl.extend({
            classNames: segmentedControlClasses,
            defaultProps: {
                radius: "md",
            },
        }),
        ActionIcon: {
            defaultProps: {
                variant: "subtle",
                radius: "md",
                size: "md",
            },
        },
        Card: {
            defaultProps: {
                radius: "lg",
                shadow: "xs",
                padding: "md",
            },
        },
        Paper: {
            defaultProps: {
                radius: "lg",
                shadow: "xs",
            },
        },
        Modal: {
            defaultProps: {
                radius: "xl",
                centered: true,
                overlayProps: {
                    blur: 8,
                    opacity: 0.4,
                },
            },
        },
        Select: {
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        },
        Tabs: {
            defaultProps: {
                variant: "outline",
            },
        },
        Table: {
            defaultProps: {
                highlightOnHover: true,
                verticalSpacing: "sm",
                horizontalSpacing: "md",
            },
        },
        Divider: {
            styles: {
                root: {
                    borderColor: "var(--octo-border-subtle)",
                },
            },
        },
        Tooltip: {
            defaultProps: {
                radius: "sm",
                offset: 6,
            },
        },
        Loader: {
            defaultProps: {
                color: "primary",
            },
        },
    },
});

export const resolver: CSSVariablesResolver = (theme) => {
    const baseVariables = {
        ...generateCSSVariables(theme.other.brand, "--octo-brand"),
        ...generateCSSVariables(theme.other.typography, "--octo-typography"),
        ...generateCSSVariables(theme.other.spacing, "--octo-spacing"),
        ...generateCSSVariables(theme.other.radius, "--octo-radius"),
        ...generateCSSVariables(theme.other.shadow, "--octo-shadow"),
        ...generateCSSVariables(theme.other.motion, "--octo-motion"),
    };

    return {
        variables: {
            ...baseVariables,
        },
        light: generateCSSVariables(theme.other.scheme.light, "--octo"),
        dark: generateCSSVariables(theme.other.scheme.dark, "--octo"),
    };
};

export { palette, tokens };
