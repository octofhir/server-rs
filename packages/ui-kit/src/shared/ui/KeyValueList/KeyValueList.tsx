import type { ReactNode } from "react";
import classes from "./KeyValueList.module.css";

export interface KeyValueListItem {
    id: string;
    label: ReactNode;
    value: ReactNode;
    caption?: ReactNode;
}

export interface KeyValueListProps {
    items: KeyValueListItem[];
}

export function KeyValueList({ items }: KeyValueListProps) {
    return (
        <dl className={classes.root}>
            {items.map((item) => (
                <div key={item.id} className={classes.item}>
                    <dt className={classes.label}>{item.label}</dt>
                    <dd className={classes.value}>{item.value}</dd>
                    {item.caption ? <dd className={classes.caption}>{item.caption}</dd> : null}
                </div>
            ))}
        </dl>
    );
}
