import { forwardRef } from "react";

type ThemeColor = "primary" | "positive" | "warning" | "danger" | "neutral";
type ThemeView = "normal" | "light" | "outlined";

const COLOR_ALIAS: Record<string, ThemeColor> = {
    primary: "primary",
    brand: "primary",
    blue: "primary",
    indigo: "primary",
    positive: "positive",
    green: "positive",
    teal: "positive",
    success: "positive",
    warning: "warning",
    yellow: "warning",
    amber: "warning",
    warm: "warning",
    orange: "warning",
    danger: "danger",
    red: "danger",
    fire: "danger",
    neutral: "neutral",
    gray: "neutral",
    grey: "neutral",
};

const VIEW_ALIAS: Record<string, ThemeView> = {
    normal: "normal",
    filled: "normal",
    light: "light",
    subtle: "light",
    transparent: "light",
    outlined: "outlined",
    outline: "outlined",
    default: "light",
};

const NAMED_SIZE: Record<string, number> = {
    xs: 16,
    s: 20,
    sm: 20,
    m: 24,
    md: 24,
    l: 32,
    lg: 32,
    xl: 40,
};

export interface ThemeIconProps extends React.HTMLAttributes<HTMLDivElement> {
    children?: React.ReactNode;
    /** Token name or raw px diameter. */
    size?: "xs" | "s" | "sm" | "m" | "md" | "l" | "lg" | "xl" | number;
    /** Visual style; legacy `variant` (filled/subtle/outline) is mapped. */
    view?: ThemeView | "filled" | "subtle" | "outline" | "default";
    /** @deprecated alias for {@link view}. */
    variant?: ThemeIconProps["view"];
    /** Semantic color; legacy palette names (gray/fire/green/…) are mapped. */
    color?:
        | ThemeColor
        | "gray"
        | "grey"
        | "green"
        | "teal"
        | "success"
        | "fire"
        | "red"
        | "warm"
        | "amber"
        | "yellow"
        | "orange"
        | "blue"
        | "indigo"
        | "brand";
    /** Corner radius in px (default 4). */
    radius?: number;
}

export const ThemeIcon = forwardRef<HTMLDivElement, ThemeIconProps>(
    ({ children, size = "m", view, variant, color = "primary", radius = 4, style, ...props }, ref) => {
        const d = typeof size === "number" ? size : NAMED_SIZE[size] ?? 24;
        const c = COLOR_ALIAS[color] ?? "primary";
        const v = VIEW_ALIAS[(view ?? variant ?? "light") as string] ?? "light";

        let bg = "transparent";
        let text = "var(--octo-text-primary)";
        let border = "1px solid transparent";

        if (v === "light") {
            if (c === "primary") { bg = "var(--octo-accent-primary-bg)"; text = "var(--octo-accent-primary)"; }
            else if (c === "positive") { bg = "var(--octo-brand-success-hover)"; text = "var(--octo-accent-positive)"; }
            else if (c === "warning") { bg = "var(--octo-brand-warm-hover)"; text = "var(--octo-accent-warm)"; }
            else if (c === "danger") { bg = "var(--octo-accent-fire-hover)"; text = "var(--octo-accent-fire)"; }
            else { bg = "var(--octo-surface-3)"; }
        } else if (v === "normal") {
            if (c === "primary") { bg = "var(--octo-accent-primary)"; text = "var(--octo-text-inverse)"; }
            else if (c === "positive") { bg = "var(--octo-accent-positive)"; text = "var(--octo-text-inverse)"; }
            else if (c === "warning") { bg = "var(--octo-accent-warm)"; text = "var(--octo-text-inverse)"; }
            else if (c === "danger") { bg = "var(--octo-accent-fire)"; text = "var(--octo-text-inverse)"; }
            else { bg = "var(--octo-surface-3)"; }
        } else {
            if (c === "primary") { border = "1px solid var(--octo-accent-primary)"; text = "var(--octo-accent-primary)"; }
            else if (c === "positive") { border = "1px solid var(--octo-accent-positive)"; text = "var(--octo-accent-positive)"; }
            else if (c === "warning") { border = "1px solid var(--octo-accent-warm)"; text = "var(--octo-accent-warm)"; }
            else if (c === "danger") { border = "1px solid var(--octo-accent-fire)"; text = "var(--octo-accent-fire)"; }
            else { border = "1px solid var(--octo-border-subtle)"; }
        }

        return (
            <div
                ref={ref}
                style={{
                    display: "inline-flex",
                    alignItems: "center",
                    justifyContent: "center",
                    width: d,
                    height: d,
                    borderRadius: radius,
                    backgroundColor: bg,
                    color: text,
                    border,
                    ...style,
                }}
                {...props}
            >
                {children}
            </div>
        );
    }
);
ThemeIcon.displayName = "ThemeIcon";
