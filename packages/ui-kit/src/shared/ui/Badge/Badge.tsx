import { forwardRef, type CSSProperties, type ReactNode } from "react";
import { Label, type LabelProps } from "@gravity-ui/uikit";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";

type LegacyBadgeColor =
    | "primary"
    | "blue"
    | "deep"
    | "indigo"
    | "green"
    | "red"
    | "fire"
    | "orange"
    | "yellow"
    | "warm"
    | "gray"
    | string;

export interface BadgeProps extends Omit<LabelProps, "theme" | "icon" | "size"> {
    theme?: LabelProps["theme"];
    size?: LabelProps["size"] | "sm" | "md" | "lg";
    color?: LegacyBadgeColor;
    variant?: "filled" | "light" | "outline" | "dot" | string;
    leftSection?: ReactNode;
    rightSection?: ReactNode;
    radius?: string | number;
    circle?: boolean;
    style?: CSSProperties;
    w?: number | string;
    h?: number | string;
    p?: number | string;
    px?: number | string;
    py?: number | string;
    pt?: number | string;
    pb?: number | string;
    pl?: number | string;
    pr?: number | string;
    m?: number | string;
    mx?: number | string;
    my?: number | string;
    mt?: number | string;
    mb?: number | string;
    ml?: number | string;
    mr?: number | string;
}

const THEME_BY_COLOR: Record<string, LabelProps["theme"]> = {
    primary: "info",
    blue: "info",
    deep: "utility",
    indigo: "utility",
    green: "success",
    red: "danger",
    fire: "danger",
    orange: "warning",
    yellow: "warning",
    warm: "warning",
    gray: "unknown",
};

function mapTheme(color?: LegacyBadgeColor, theme?: LabelProps["theme"]) {
    return theme ?? (color ? THEME_BY_COLOR[color] : undefined) ?? "normal";
}

function mapSize(size?: BadgeProps["size"]): LabelProps["size"] {
    if (size === "lg") return "m";
    if (size === "md") return "m";
    if (size === "sm") return "s";
    return size;
}

export const Badge = forwardRef<HTMLDivElement, BadgeProps>(function Badge(
    {
        color,
        theme,
        variant: _variant,
        leftSection,
        rightSection,
        radius,
        circle,
        size,
        style,
        children,
        ...props
    },
    ref,
) {
    const LabelAny = Label as unknown as React.ComponentType<Record<string, unknown>>;
    const cleaned = cleanLayoutProps(props);
    const mergedStyle: CSSProperties = {
        ...getSpacingStyles(props),
        ...(circle ? { borderRadius: 999 } : {}),
        ...(radius !== undefined ? { borderRadius: radius } : {}),
        ...style,
    };

    return (
        <LabelAny
            ref={ref}
            theme={mapTheme(color, theme)}
            size={mapSize(size)}
            icon={leftSection}
            style={mergedStyle}
            {...cleaned}
        >
            {children}
            {rightSection}
        </LabelAny>
    );
});
