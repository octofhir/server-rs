import { forwardRef, type CSSProperties } from "react";
import styles from "./Skeleton.module.css";

export interface SkeletonProps extends React.HTMLAttributes<HTMLDivElement> {
    /** Width (number => px). */
    w?: number | string;
    /** Height (number => px). Defaults to 1em. */
    h?: number | string;
    /** Render as a circle (use equal w/h). */
    circle?: boolean;
    /** Corner radius override. */
    radius?: number | string;
}

function size(v: number | string | undefined): string | undefined {
    if (v == null) return undefined;
    return typeof v === "number" ? `${v}px` : v;
}

export const Skeleton = forwardRef<HTMLDivElement, SkeletonProps>(function Skeleton(
    { w, h = "1em", circle, radius, className, style, ...props },
    ref,
) {
    const merged: CSSProperties = {
        width: size(w) ?? "100%",
        height: size(h),
        borderRadius: radius != null ? size(radius) : undefined,
        ...style,
    };
    return (
        <div
            ref={ref}
            className={[styles.skeleton, className].filter(Boolean).join(" ")}
            data-circle={circle ? "true" : undefined}
            aria-hidden="true"
            style={merged}
            {...props}
        />
    );
});
