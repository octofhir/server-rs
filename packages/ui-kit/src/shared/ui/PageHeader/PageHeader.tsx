import type { ReactNode } from "react";
import { Button, type ButtonProps } from "../Button";
import classes from "./PageHeader.module.css";

export interface PageHeaderAction {
    id: string;
    label: ReactNode;
    icon?: ReactNode;
    view?: ButtonProps["view"];
    onClick?: () => void;
}

export interface PageHeaderProps {
    eyebrow?: ReactNode;
    title: ReactNode;
    description?: ReactNode;
    actions?: PageHeaderAction[];
}

export function PageHeader({ eyebrow, title, description, actions }: PageHeaderProps) {
    return (
        <header className={classes.root}>
            <div className={classes.content}>
                {eyebrow ? <div className={classes.eyebrow}>{eyebrow}</div> : null}
                <h1 className={classes.title}>{title}</h1>
                {description ? <div className={classes.description}>{description}</div> : null}
            </div>

            {actions?.length ? (
                <div className={classes.actions}>
                    {actions.map((action) => (
                        <Button
                            key={action.id}
                            view={action.view ?? "normal"}
                            size="m"
                            onClick={action.onClick}
                        >
                            {action.icon ? <Button.Icon>{action.icon}</Button.Icon> : null}
                            {action.label}
                        </Button>
                    ))}
                </div>
            ) : null}
        </header>
    );
}
