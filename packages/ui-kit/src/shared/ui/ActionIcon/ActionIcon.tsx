import { forwardRef, type CSSProperties, type ReactNode } from "react";
import { Button, type ButtonProps } from "../Button";

/**
 * Square icon button — wraps Gravity `Button` with matching width/height
 * sizing so a single icon fits centered, with `pin="round-round"`.
 *
 * Accepts the full Gravity ButtonProps surface; intended ref is button-like.
 */
export type ActionIconProps = Omit<ButtonProps, "href"> & {
    children?: ReactNode;
};

const SQUARE: Record<string, number> = { xs: 22, s: 24, m: 28, l: 36, xl: 42 };

export const ActionIcon = forwardRef<HTMLButtonElement, ActionIconProps>(
    ({ view = "flat", size = "m", style, ...props }, ref) => {
        const dim = SQUARE[size as string] ?? 28;
        const merged: CSSProperties = {
            width: dim,
            minWidth: dim,
            height: dim,
            minHeight: dim,
            padding: 0,
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "center",
            ...style,
        };
        return (
            <Button
                ref={ref}
                view={view}
                size={size}
                pin="round-round"
                style={merged}
                {...props}
            />
        );
    },
);
ActionIcon.displayName = "ActionIcon";
