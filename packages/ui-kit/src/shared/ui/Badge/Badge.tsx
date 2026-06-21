import { forwardRef, type CSSProperties, type ReactNode } from "react";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";
import styles from "./Badge.module.css";

type BadgeTone = "primary" | "success" | "danger" | "warning" | "info" | "neutral";
type BadgeThemeName = "normal" | "info" | "success" | "warning" | "danger" | "utility" | "unknown" | "clear";

type BadgeColorName =
    | "primary" | "blue" | "deep" | "indigo" | "green" | "red" | "fire"
    | "orange" | "yellow" | "warm" | "gray" | string;

export interface BadgeProps extends Omit<React.HTMLAttributes<HTMLSpanElement>, "color"> {
    /** Semantic theme name; resolved to a tone. */
    theme?: BadgeThemeName;
    size?: "xs" | "s" | "m" | "sm" | "md" | "lg";
    /** Palette name; resolved to a semantic tone. */
    color?: BadgeColorName;
    variant?: "filled" | "light" | "outline" | "dot" | string;
    leftSection?: ReactNode;
    rightSection?: ReactNode;
    radius?: string | number;
    circle?: boolean;
    children?: ReactNode;
    w?: number | string; h?: number | string;
    p?: number | string; px?: number | string; py?: number | string;
    pt?: number | string; pb?: number | string; pl?: number | string; pr?: number | string;
    m?: number | string; mx?: number | string; my?: number | string;
    mt?: number | string; mb?: number | string; ml?: number | string; mr?: number | string;
}

const TONE_BY_COLOR: Record<string, BadgeTone> = {
    primary: "primary",
    blue: "info", deep: "info", indigo: "info",
    green: "success",
    red: "danger", fire: "danger",
    orange: "warning", yellow: "warning", warm: "warning",
    gray: "neutral",
};

const TONE_BY_THEME: Record<string, BadgeTone> = {
    normal: "neutral",
    clear: "neutral",
    unknown: "neutral",
    utility: "info",
    info: "info",
    success: "success",
    warning: "warning",
    danger: "danger",
};

const SIZE_ALIAS: Record<string, "xs" | "s" | "m"> = {
    xs: "xs", s: "s", sm: "s", m: "m", md: "m", lg: "m",
};

function resolveTone(color?: BadgeColorName, theme?: BadgeThemeName): BadgeTone {
    if (color && TONE_BY_COLOR[color]) return TONE_BY_COLOR[color];
    if (theme && TONE_BY_THEME[theme]) return TONE_BY_THEME[theme];
    return "primary";
}

export const Badge = forwardRef<HTMLSpanElement, BadgeProps>(function Badge(
    {
        color,
        theme,
        variant = "light",
        size = "s",
        leftSection,
        rightSection,
        radius,
        circle,
        style,
        className,
        children,
        ...props
    },
    ref,
) {
    const cleaned = cleanLayoutProps(props);
    const mergedStyle: CSSProperties = {
        ...getSpacingStyles(props),
        ...(circle ? { borderRadius: 9999 } : {}),
        ...(radius !== undefined ? { borderRadius: radius } : {}),
        ...style,
    };

    return (
        <span
            ref={ref}
            className={[styles.badge, className].filter(Boolean).join(" ")}
            data-tone={resolveTone(color, theme)}
            data-variant={variant}
            data-size={SIZE_ALIAS[size] ?? "s"}
            style={mergedStyle}
            {...cleaned}
        >
            {leftSection != null && <span className={styles.icon}>{leftSection}</span>}
            {children}
            {rightSection != null && <span className={styles.icon}>{rightSection}</span>}
        </span>
    );
});
