import type { ReactNode } from "react";
import { StatusBadge, type StatusTone } from "../StatusBadge";
import classes from "./RecordList.module.css";

export interface RecordListMetaItem {
    id: string;
    label: ReactNode;
    tone?: StatusTone;
}

export interface RecordListItem {
    id: string;
    title: ReactNode;
    subtitle?: ReactNode;
    description?: ReactNode;
    leading?: ReactNode;
    aside?: ReactNode;
    meta?: RecordListMetaItem[];
    disabled?: boolean;
}

export interface RecordListProps {
    items: RecordListItem[];
    selectedId?: string;
    density?: "default" | "compact";
    emptyText?: ReactNode;
    className?: string;
    onSelect?: (item: RecordListItem) => void;
}

export function RecordList({
    items,
    selectedId,
    density = "default",
    emptyText = "No records",
    className,
    onSelect,
}: RecordListProps) {
    if (!items.length) {
        return <div className={classes.empty}>{emptyText}</div>;
    }

    return (
        <div className={[classes.root, className].filter(Boolean).join(" ")}>
            {items.map((item) => {
                const Element = onSelect && !item.disabled ? "button" : "div";

                return (
                    <Element
                        key={item.id}
                        type={Element === "button" ? "button" : undefined}
                        disabled={Element === "button" ? item.disabled : undefined}
                        className={[
                            classes.item,
                            density === "compact" ? classes.compact : undefined,
                            Element === "button" ? classes.itemButton : undefined,
                            item.id === selectedId ? classes.selected : undefined,
                            item.disabled ? classes.disabled : undefined,
                        ]
                            .filter(Boolean)
                            .join(" ")}
                        onClick={Element === "button" ? () => onSelect?.(item) : undefined}
                    >
                        {item.leading ? <div className={classes.leading}>{item.leading}</div> : null}
                        <div className={classes.body}>
                            <div className={classes.titleRow}>
                                <div className={classes.title}>{item.title}</div>
                                {item.subtitle ? <div className={classes.subtitle}>{item.subtitle}</div> : null}
                            </div>
                            {item.description ? (
                                <div className={classes.description}>{item.description}</div>
                            ) : null}
                            {item.meta?.length ? (
                                <div className={classes.meta}>
                                    {item.meta.map((meta) => (
                                        <StatusBadge key={meta.id} tone={meta.tone ?? "neutral"}>
                                            {meta.label}
                                        </StatusBadge>
                                    ))}
                                </div>
                            ) : null}
                        </div>
                        {item.aside ? <div className={classes.aside}>{item.aside}</div> : null}
                    </Element>
                );
            })}
        </div>
    );
}
