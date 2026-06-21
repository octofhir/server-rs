import { forwardRef, type CSSProperties, type ElementType, type ReactNode } from "react";
import styles from "./Text.module.css";

export type TextVariant =
    | "display-1"
    | "header-1" | "header-2"
    | "subheader-1" | "subheader-2" | "subheader-3"
    | "body-1" | "body-2" | "body-3"
    | "caption-1" | "caption-2"
    | "code-1" | "code-2";

export type TextColor =
    | "primary" | "secondary" | "muted" | "hint"
    | "danger" | "success" | "warning" | "info" | "brand" | "inherit";

const COLOR_ALIAS: Record<string, TextColor> = {
    primary: "primary",
    secondary: "secondary",
    dimmed: "muted",
    muted: "muted",
    hint: "muted",
    red: "danger", fire: "danger", danger: "danger",
    green: "success", teal: "success", positive: "success", success: "success",
    yellow: "warning", warm: "warning", orange: "warning", warning: "warning",
    blue: "info", info: "info",
    brand: "brand", indigo: "brand",
    inherit: "inherit",
};

const SIZE_TO_VARIANT: Record<string, TextVariant> = {
    xs: "caption-2",
    sm: "body-2",
    md: "body-1",
    lg: "subheader-2",
    xl: "header-2",
};

export interface TextProps extends Omit<React.HTMLAttributes<HTMLElement>, "color"> {
    as?: ElementType;
    variant?: TextVariant;
    color?: TextColor | string;
    ellipsis?: boolean;
    children?: ReactNode;

    /** Size shorthand; mapped to `variant`. */
    size?: "xs" | "sm" | "md" | "lg" | "xl" | string;
    /** Color shorthand; mapped to `color`. */
    c?: string;
    /** Font weight. */
    fw?: number;
    /** Font family; `"monospace"` applies the mono family. */
    ff?: string;
    /** Text alignment. */
    ta?: CSSProperties["textAlign"];
    /** Truncate with an ellipsis. */
    truncate?: boolean;
}

export const Text = forwardRef<HTMLElement, TextProps>(function Text(
    {
        as,
        variant,
        color,
        ellipsis,
        size,
        c,
        fw,
        ff,
        ta,
        truncate,
        className,
        style,
        children,
        ...props
    },
    ref,
) {
    const Component = (as ?? "span") as ElementType;
    const resolvedVariant = variant ?? (size ? SIZE_TO_VARIANT[size] ?? "body-1" : undefined);
    const rawColor = color ?? c;
    const resolvedColor = rawColor ? COLOR_ALIAS[rawColor] ?? (rawColor as TextColor) : undefined;

    const dynamicStyle: CSSProperties = {
        ...(fw != null ? { fontWeight: fw } : {}),
        ...(ff === "monospace" ? { fontFamily: "var(--octo-typography-mono, monospace)" } : {}),
        ...(ta ? { textAlign: ta } : {}),
        ...style,
    };

    return (
        <Component
            ref={ref}
            className={[styles.text, className].filter(Boolean).join(" ")}
            data-variant={resolvedVariant}
            data-color={resolvedColor && resolvedColor !== "primary" ? resolvedColor : undefined}
            data-ellipsis={ellipsis || truncate ? "true" : undefined}
            style={dynamicStyle}
            {...props}
        >
            {children}
        </Component>
    );
});
