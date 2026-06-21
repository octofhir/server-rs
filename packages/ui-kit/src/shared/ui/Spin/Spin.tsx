import { forwardRef } from "react";
import type { Size } from "../layout-utils";
import styles from "./Spin.module.css";

export interface SpinProps extends React.HTMLAttributes<HTMLSpanElement> {
    size?: Size;
}

export const Spin = forwardRef<HTMLSpanElement, SpinProps>(function Spin(
    { size = "md", className, ...props },
    ref,
) {
    return (
        <span
            ref={ref}
            className={[styles.spin, className].filter(Boolean).join(" ")}
            data-size={size}
            role="status"
            aria-label="Loading"
            {...props}
        />
    );
});
