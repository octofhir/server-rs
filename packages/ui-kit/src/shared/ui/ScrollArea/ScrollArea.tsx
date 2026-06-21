import { forwardRef, type CSSProperties, type ReactNode } from "react";
import { cleanLayoutProps, getSpacingStyles, type SpacingProps } from "../layout-utils";

export interface ScrollAreaProps
    extends React.HTMLAttributes<HTMLDivElement>,
        SpacingProps {
    children?: ReactNode;
    /** Max height (enables vertical scroll past it). */
    mah?: number | string;
    /** Max width. */
    maw?: number | string;
    /** Scroll behaviour (accepted for API parity; visual behaviour unchanged). */
    type?: "auto" | "always" | "scroll" | "hover" | "never" | string;
    /** Scroll axis. */
    axis?: "vertical" | "horizontal" | "both";
}

/**
 * Overflow-scroll container. `ScrollArea.Autosize` grows with content up to
 * `mah`, then scrolls.
 */
export const ScrollArea = forwardRef<HTMLDivElement, ScrollAreaProps>(function ScrollArea(
    { children, mah, maw, type: _type, axis = "vertical", style, ...rest },
    ref,
) {
    const props = cleanLayoutProps(rest);
    const overflow: CSSProperties =
        axis === "both"
            ? { overflow: "auto" }
            : axis === "horizontal"
              ? { overflowX: "auto", overflowY: "hidden" }
              : { overflowY: "auto", overflowX: "hidden" };

    return (
        <div
            ref={ref}
            style={{
                ...getSpacingStyles(rest),
                ...(mah != null ? { maxHeight: mah } : {}),
                ...(maw != null ? { maxWidth: maw } : {}),
                ...overflow,
                ...style,
            }}
            {...props}
        >
            {children}
        </div>
    );
}) as React.ForwardRefExoticComponent<
    ScrollAreaProps & React.RefAttributes<HTMLDivElement>
> & { Autosize: typeof ScrollAreaAutosize };

/** Grows with content up to `mah`, then scrolls. */
const ScrollAreaAutosize = forwardRef<HTMLDivElement, ScrollAreaProps>(function ScrollAreaAutosize(
    props,
    ref,
) {
    return <ScrollArea ref={ref} {...props} />;
});

ScrollArea.Autosize = ScrollAreaAutosize;
