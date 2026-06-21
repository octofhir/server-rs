import { forwardRef } from "react";
import styles from "./Spin.module.css";

export interface SpinProps extends React.HTMLAttributes<HTMLSpanElement> {
    size?: "xs" | "s" | "sm" | "m" | "l" | "xl";
}

const SIZE_ALIAS: Record<string, "xs" | "s" | "m" | "l" | "xl"> = {
    xs: "xs", s: "s", sm: "s", m: "m", l: "l", xl: "xl",
};

export const Spin = forwardRef<HTMLSpanElement, SpinProps>(function Spin(
    { size = "m", className, ...props },
    ref,
) {
    return (
        <span
            ref={ref}
            className={[styles.spin, className].filter(Boolean).join(" ")}
            data-size={SIZE_ALIAS[size] ?? "m"}
            role="status"
            aria-label="Loading"
            {...props}
        />
    );
});
