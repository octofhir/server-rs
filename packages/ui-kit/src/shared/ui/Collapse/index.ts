import React, { forwardRef } from "react";

export interface CollapseProps extends React.HTMLAttributes<HTMLDivElement> {
    in?: boolean;
    transitionDuration?: number;
}

/**
 * Animated open/close container. Uses the `grid-template-rows: 0fr → 1fr`
 * technique so it transitions height without measuring, honoring
 * `transitionDuration` (ms). Content is removed from the a11y tree when closed.
 */
export const Collapse = forwardRef<HTMLDivElement, CollapseProps>(
    ({ in: opened = false, children, style, transitionDuration = 200, ...props }, ref) => {
        return React.createElement(
            "div",
            {
                ref,
                "aria-hidden": opened ? undefined : true,
                style: {
                    display: "grid",
                    gridTemplateRows: opened ? "1fr" : "0fr",
                    transition: `grid-template-rows ${transitionDuration}ms ease`,
                    ...style,
                },
                ...props,
            },
            React.createElement(
                "div",
                {
                    style: {
                        overflow: "hidden",
                        minHeight: 0,
                        visibility: opened ? undefined : "hidden",
                    },
                },
                children,
            ),
        );
    },
);

Collapse.displayName = "Collapse";
