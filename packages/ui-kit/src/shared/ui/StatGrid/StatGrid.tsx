import type { ReactNode } from "react";
import type { StatusTone } from "../StatusBadge";
import classes from "./StatGrid.module.css";

export interface StatGridItem {
    id: string;
    label: ReactNode;
    value: ReactNode;
    caption?: ReactNode;
    tone?: StatusTone;
}

export interface StatGridProps {
    items: StatGridItem[];
    className?: string;
}

const toneClassName: Record<StatusTone, string> = {
    neutral: classes.toneNeutral,
    info: classes.toneInfo,
    success: classes.toneSuccess,
    warning: classes.toneWarning,
    danger: classes.toneDanger,
};

export function StatGrid({ items, className }: StatGridProps) {
    return (
        <dl className={[classes.root, className].filter(Boolean).join(" ")}>
            {items.map((item) => (
                <div
                    key={item.id}
                    className={[classes.item, toneClassName[item.tone ?? "neutral"]].join(" ")}
                >
                    <dt className={classes.label}>{item.label}</dt>
                    <dd className={classes.value}>{item.value}</dd>
                    {item.caption ? <dd className={classes.caption}>{item.caption}</dd> : null}
                </div>
            ))}
        </dl>
    );
}
