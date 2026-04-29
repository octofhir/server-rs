import React, { forwardRef } from "react";

export interface CollapseProps extends React.HTMLAttributes<HTMLDivElement> {
    in?: boolean;
    transitionDuration?: number;
}

export const Collapse = forwardRef<HTMLDivElement, CollapseProps>(
    ({ in: opened = false, children, style, transitionDuration: _transitionDuration, ...props }, ref) => {
        return React.createElement(
            "div",
            {
                ref,
                hidden: !opened,
                style: { display: opened ? undefined : "none", ...style },
                ...props,
            },
            children,
        );
    },
);

Collapse.displayName = "Collapse";
