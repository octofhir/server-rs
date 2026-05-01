import type { ReactNode } from "react";
import { StatusBadge, type StatusTone } from "../StatusBadge";
import classes from "./CommandCard.module.css";

export interface CommandCardMeta {
    id: string;
    label: ReactNode;
    tone?: StatusTone;
}

export interface CommandCardProps {
    title: ReactNode;
    description?: ReactNode;
    icon?: ReactNode;
    status?: ReactNode;
    statusTone?: StatusTone;
    meta?: CommandCardMeta[];
    onClick?: () => void;
    disabled?: boolean;
}

export function CommandCard({
    title,
    description,
    icon,
    status,
    statusTone = "neutral",
    meta,
    onClick,
    disabled,
}: CommandCardProps) {
    const content = (
        <>
            <div className={classes.top}>
                {icon ? <span className={classes.icon}>{icon}</span> : null}
                <span className={classes.title}>{title}</span>
                {status ? <StatusBadge tone={statusTone}>{status}</StatusBadge> : null}
            </div>

            {description ? <div className={classes.description}>{description}</div> : null}

            {meta?.length ? (
                <div className={classes.meta}>
                    {meta.map((item) => (
                        <StatusBadge key={item.id} tone={item.tone ?? "neutral"}>
                            {item.label}
                        </StatusBadge>
                    ))}
                </div>
            ) : null}
        </>
    );

    if (!onClick) {
        return <div className={classes.root}>{content}</div>;
    }

    return (
        <button
            className={`${classes.root} ${classes.button}`}
            type="button"
            onClick={onClick}
            disabled={disabled}
        >
            {content}
        </button>
    );
}
