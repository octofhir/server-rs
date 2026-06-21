import { forwardRef } from "react";
import styles from "./Link.module.css";

export interface LinkProps extends React.AnchorHTMLAttributes<HTMLAnchorElement> {
    /** Visual emphasis. */
    view?: "normal" | "primary" | "secondary";
}

export const Link = forwardRef<HTMLAnchorElement, LinkProps>(function Link(
    { view = "normal", className, children, ...props },
    ref,
) {
    return (
        <a
            ref={ref}
            data-view={view}
            className={[styles.link, className].filter(Boolean).join(" ")}
            {...props}
        >
            {children}
        </a>
    );
});
