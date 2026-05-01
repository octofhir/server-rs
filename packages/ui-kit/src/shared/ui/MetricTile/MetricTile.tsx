import type { ReactNode } from "react";
import { Surface } from "../Surface";
import classes from "./MetricTile.module.css";

export interface MetricTileProps {
    title: ReactNode;
    value: ReactNode;
    caption?: ReactNode;
    icon?: ReactNode;
}

export function MetricTile({ title, value, caption, icon }: MetricTileProps) {
    return (
        <Surface className={classes.root} view="outlined" padding="m">
            <div className={classes.body}>
                <div className={classes.title}>{title}</div>
                <div>
                    <div className={classes.value}>{value}</div>
                    {caption ? <div className={classes.caption}>{caption}</div> : null}
                </div>
            </div>
            {icon ? <div className={classes.icon}>{icon}</div> : null}
        </Surface>
    );
}
