import { forwardRef, type CSSProperties, type ReactNode } from "react";

export interface ScrollAreaProps extends React.HTMLAttributes<HTMLDivElement> {
    children?: ReactNode;
    /** Fixed height. */
    h?: number | string;
    /** Max height (enables vertical scroll past it). */
    mah?: number | string;
    /** Max width. */
    maw?: number | string;
    /** Scroll axis. */
    axis?: "vertical" | "horizontal" | "both";
}

function toCss(v: number | string | undefined): string | undefined {
    if (v == null) return undefined;
    return typeof v === "number" ? `${v}px` : v;
}

/**
 * Overflow-scroll container. `ScrollArea.Autosize` grows with content up to
 * `mah`, then scrolls.
 */
export const ScrollArea = forwardRef<HTMLDivElement, ScrollAreaProps>(function ScrollArea(
    { children, h, mah, maw, axis = "vertical", style, ...props },
    ref,
) {
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
                height: toCss(h),
                maxHeight: toCss(mah),
                maxWidth: toCss(maw),
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
