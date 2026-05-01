import { forwardRef, type HTMLAttributes } from "react";
import classes from "./Surface.module.css";

export interface SurfaceProps extends HTMLAttributes<HTMLDivElement> {
    view?: "plain" | "filled" | "tinted" | "outlined";
    padding?: "none" | "xs" | "s" | "m" | "l";
    interactive?: boolean;
}

const viewClassName = {
    plain: classes.viewPlain,
    filled: classes.viewFilled,
    tinted: classes.viewTinted,
    outlined: classes.viewOutlined,
};

const paddingClassName = {
    none: classes.paddingNone,
    xs: classes.paddingXs,
    s: classes.paddingS,
    m: classes.paddingM,
    l: classes.paddingL,
};

export const Surface = forwardRef<HTMLDivElement, SurfaceProps>(function Surface(
    { view = "outlined", padding = "m", interactive = false, className, ...props },
    ref,
) {
    return (
        <div
            ref={ref}
            className={[
                classes.surface,
                viewClassName[view],
                paddingClassName[padding],
                interactive ? classes.interactive : undefined,
                className,
            ]
                .filter(Boolean)
                .join(" ")}
            {...props}
        />
    );
});
