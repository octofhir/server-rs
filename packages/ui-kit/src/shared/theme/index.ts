import {
    Alert,
    createTheme,
    virtualColor,
    type MantineThemeOverride,
    type CSSVariablesResolver,
    ActionIcon,
    Badge,
    Button,
    Card,
    Checkbox,
    Menu,
    Modal,
    MultiSelect,
    NavLink,
    NumberInput,
    Paper,
    PasswordInput,
    Select,
    Switch,
    Table,
    Tabs,
    Textarea,
    TextInput,
    ThemeIcon,
    SegmentedControl,
} from "@mantine/core";
import { DateInput, DateTimePicker } from "@mantine/dates";
import { palette } from "./colors";
import { tokens, type OctoTokens } from "./tokens";
import { generateCSSVariables } from "./utils";

import alertClasses from "../ui/Alert/Alert.module.css";
import actionIconClasses from "../ui/ActionIcon/ActionIcon.module.css";
import badgeClasses from "../ui/Badge/Badge.module.css";
import buttonClasses from "../ui/Button/Button.module.css";
import cardClasses from "../ui/Card/Card.module.css";
import checkboxClasses from "../ui/Checkbox/Checkbox.module.css";
import menuClasses from "../ui/Menu/Menu.module.css";
import modalClasses from "../ui/Modal/Modal.module.css";
import multiSelectClasses from "../ui/MultiSelect/MultiSelect.module.css";
import navLinkClasses from "../ui/NavLink/NavLink.module.css";
import paperClasses from "../ui/Paper/Paper.module.css";
import selectClasses from "../ui/Select/Select.module.css";
import switchClasses from "../ui/Switch/Switch.module.css";
import tableClasses from "../ui/Table/Table.module.css";
import tabsClasses from "../ui/Tabs/Tabs.module.css";
import textInputClasses from "../ui/TextInput/TextInput.module.css";
import themeIconClasses from "../ui/ThemeIcon/ThemeIcon.module.css";
import segmentedControlClasses from "../ui/SegmentedControl/SegmentedControl.module.css";

export type OctoThemeOther = OctoTokens;

declare module "@mantine/core" {
    export interface MantineThemeOther extends OctoThemeOther { }
}

export const theme: MantineThemeOverride = createTheme({
    primaryColor: "primary",
    primaryShade: { light: 5, dark: 4 },
    defaultGradient: { from: "primary.4", to: "warm.4", deg: 122 },
    colors: {
        ...palette,
        success: virtualColor({ name: "success", dark: "primary", light: "primary" }),
        warning: virtualColor({ name: "warning", dark: "warm", light: "warm" }),
        error: virtualColor({ name: "error", dark: "fire", light: "fire" }),
    },
    fontFamily: tokens.typography.family,
    fontFamilyMonospace: tokens.typography.mono,
    headings: {
        fontFamily: tokens.typography.heading,
        fontWeight: `${tokens.typography.weight.semibold}`,
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
        ActionIcon: ActionIcon.extend({
            classNames: actionIconClasses,
            defaultProps: {
                variant: "subtle",
                radius: "md",
                size: "md",
            },
        }),
        Card: Card.extend({
            classNames: cardClasses,
            defaultProps: {
                radius: "lg",
                shadow: "xs",
                padding: "md",
            },
        }),
        Paper: Paper.extend({
            classNames: paperClasses,
            defaultProps: {
                radius: "lg",
                shadow: "xs",
            },
        }),
        Modal: Modal.extend({
            classNames: modalClasses,
            defaultProps: {
                radius: "xl",
                centered: true,
                overlayProps: {
                    blur: 8,
                    opacity: 0.4,
                },
            },
        }),
        Menu: Menu.extend({
            classNames: menuClasses,
            defaultProps: {
                radius: "md",
                shadow: "md",
            },
        }),
        NavLink: NavLink.extend({
            classNames: navLinkClasses,
            defaultProps: {
                variant: "subtle",
            },
        }),
        Checkbox: Checkbox.extend({
            classNames: checkboxClasses,
            defaultProps: {
                radius: "sm",
            },
        }),
        Switch: Switch.extend({
            classNames: switchClasses,
            defaultProps: {
                radius: "xl",
            },
        }),
        Alert: Alert.extend({
            classNames: alertClasses,
            defaultProps: {
                radius: "md",
                variant: "light",
            },
        }),
        ThemeIcon: ThemeIcon.extend({
            classNames: themeIconClasses,
            defaultProps: {
                variant: "light",
                radius: "md",
            },
        }),
        Select: Select.extend({
            classNames: selectClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        MultiSelect: MultiSelect.extend({
            classNames: multiSelectClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        Tabs: Tabs.extend({
            classNames: tabsClasses,
            defaultProps: {
                variant: "outline",
            },
        }),
        Table: Table.extend({
            classNames: tableClasses,
            defaultProps: {
                highlightOnHover: true,
                verticalSpacing: "sm",
                horizontalSpacing: "md",
            },
        }),
        Textarea: Textarea.extend({
            classNames: textInputClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        NumberInput: NumberInput.extend({
            classNames: textInputClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        PasswordInput: PasswordInput.extend({
            classNames: textInputClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        DateInput: DateInput.extend({
            classNames: selectClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
        DateTimePicker: DateTimePicker.extend({
            classNames: selectClasses,
            defaultProps: {
                radius: "md",
                size: "sm",
            },
        }),
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
        ScrollArea: {
            defaultProps: {
                scrollbarSize: 5,
                type: "hover",
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
