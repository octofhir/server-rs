import type { HTMLAttributes } from "react";
import classes from "./CanonicalUri.module.css";

export interface CanonicalUriProps extends HTMLAttributes<HTMLSpanElement> {
    value: string;
}

export function CanonicalUri({ value, className, title, ...props }: CanonicalUriProps) {
    return (
        <span
            className={[classes.root, className].filter(Boolean).join(" ")}
            title={title ?? value}
            {...props}
        >
            <span className={classes.value}>{value}</span>
        </span>
    );
}
