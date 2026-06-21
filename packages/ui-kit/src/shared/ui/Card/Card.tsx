import { forwardRef } from "react";
import { cleanLayoutProps, getSpacingStyles, type SpacingProps } from "../layout-utils";
import styles from "./Card.module.css";

export type CardVariant = "filled" | "outlined" | "raised" | "clear";

export interface CardProps
    extends Omit<React.HTMLAttributes<HTMLDivElement>, "style">,
        SpacingProps {
    variant?: CardVariant;
    /** Alias of {@link variant}. */
    view?: CardVariant;
    /** Shorthand for `variant="outlined"`. */
    withBorder?: boolean;
    radius?: string | number;
    /** Apply an elevation shadow regardless of variant. */
    shadow?: boolean | string;
    padding?: number | string;
    bg?: string;
    ta?: React.CSSProperties["textAlign"];
    style?: React.CSSProperties;
}

export const Card = forwardRef<HTMLDivElement, CardProps>(
    (
        { variant, view, withBorder, radius, shadow, padding, p, bg, ta, className, style, ...props },
        ref,
    ) => {
        const resolved = variant ?? view ?? (withBorder ? "outlined" : "filled");
        const mergedStyle: React.CSSProperties = {
            ...getSpacingStyles({ ...props, p: p ?? padding }),
            ...(bg ? { background: bg } : {}),
            ...(ta ? { textAlign: ta } : {}),
            ...(radius !== undefined ? { borderRadius: radius } : {}),
            ...(shadow ? { boxShadow: "var(--octo-shadow-md, 0 4px 12px rgba(0, 0, 0, 0.08))" } : {}),
            ...style,
        };
        const cleaned = cleanLayoutProps(props);
        return (
            <div
                ref={ref}
                className={[styles.card, className].filter(Boolean).join(" ")}
                data-variant={resolved}
                style={mergedStyle}
                {...cleaned}
            />
        );
    },
);
Card.displayName = "Card";
