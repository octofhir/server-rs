import type { ReactNode } from "react";
import { Surface, type SurfaceProps } from "../Surface";
import classes from "./SectionPanel.module.css";

export interface SectionPanelProps extends Omit<SurfaceProps, "title"> {
    title: ReactNode;
    description?: ReactNode;
    actions?: ReactNode;
}

export function SectionPanel({
    title,
    description,
    actions,
    children,
    className,
    ...surfaceProps
}: SectionPanelProps) {
    return (
        <Surface className={[classes.root, className].filter(Boolean).join(" ")} {...surfaceProps}>
            <div className={classes.header}>
                <div className={classes.titleBlock}>
                    <h2 className={classes.title}>{title}</h2>
                    {description ? <div className={classes.description}>{description}</div> : null}
                </div>
                {actions ? <div className={classes.actions}>{actions}</div> : null}
            </div>
            {children}
        </Surface>
    );
}
