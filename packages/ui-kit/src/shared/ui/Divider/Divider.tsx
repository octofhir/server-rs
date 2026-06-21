import { forwardRef, type ReactNode } from "react";
import styles from "./Divider.module.css";

export interface DividerProps {
    orientation?: "horizontal" | "vertical";
    /** Horizontal alignment of the optional label. */
    align?: "start" | "center" | "end";
    /** Optional centered label (horizontal dividers only). */
    children?: ReactNode;
    className?: string;
    style?: React.CSSProperties;
}

export const Divider = forwardRef<HTMLDivElement, DividerProps>(function Divider(
    { orientation = "horizontal", align = "center", children, className, style },
    ref,
) {
    if (children != null) {
        return (
            <div
                ref={ref}
                role="separator"
                data-align={align}
                className={[styles.labeled, className].filter(Boolean).join(" ")}
                style={style}
            >
                {children}
            </div>
        );
    }
    return (
        <div
            ref={ref}
            role="separator"
            aria-orientation={orientation}
            data-orientation={orientation}
            className={[styles.divider, className].filter(Boolean).join(" ")}
            style={style}
        />
    );
});
