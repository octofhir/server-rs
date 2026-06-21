import type { ComponentType, ReactNode } from "react";
import {
    Badge,
    CommandCard,
    type CommandCardMeta,
    MetricTile,
    PageHeader,
    type PageHeaderAction,
    type StatusTone,
    Surface,
} from "#/shared/ui";
import classes from "./WorkspaceBoard.module.css";

export interface WorkspaceBoardMetric {
    id: string;
    title: ReactNode;
    value: ReactNode;
    caption?: ReactNode;
    icon?: ReactNode;
}

export type WorkspaceBoardAction = PageHeaderAction;

export type WorkspaceBoardItemMeta = CommandCardMeta;

export interface WorkspaceBoardItem {
    id: string;
    title: ReactNode;
    description?: ReactNode;
    icon?: ComponentType<{ width?: number; height?: number }>;
    status?: ReactNode;
    statusTone?: StatusTone;
    meta?: WorkspaceBoardItemMeta[];
    onClick?: () => void;
}

export interface WorkspaceBoardColumn {
    id: string;
    title: ReactNode;
    caption?: ReactNode;
    items: WorkspaceBoardItem[];
    emptyLabel?: ReactNode;
}

export interface WorkspaceBoardProps {
    eyebrow?: ReactNode;
    title: ReactNode;
    description?: ReactNode;
    actions?: WorkspaceBoardAction[];
    metrics?: WorkspaceBoardMetric[];
    columns: WorkspaceBoardColumn[];
    aside?: ReactNode;
}

export function WorkspaceBoard({
    eyebrow,
    title,
    description,
    actions,
    metrics,
    columns,
    aside,
}: WorkspaceBoardProps) {
    return (
        <section className={classes.root}>
            <PageHeader
                eyebrow={eyebrow}
                title={title}
                description={description}
                actions={actions}
            />

            {metrics?.length ? (
                <div className={classes.metrics}>
                    {metrics.map((metric) => (
                        <MetricTile
                            key={metric.id}
                            title={metric.title}
                            value={metric.value}
                            caption={metric.caption}
                            icon={metric.icon}
                        />
                    ))}
                </div>
            ) : null}

            <div className={classes.content}>
                <div className={classes.board}>
                    {columns.map((column) => (
                        <section key={column.id} className={classes.column}>
                            <div className={classes.columnHeader}>
                                <div>
                                    <div className={classes.columnTitle}>{column.title}</div>
                                    {column.caption ? (
                                        <div className={classes.columnCaption}>{column.caption}</div>
                                    ) : null}
                                </div>
                                <Badge theme="unknown" size="sm">
                                    {column.items.length}
                                </Badge>
                            </div>

                            {column.items.length ? (
                                <div className={classes.items}>
                                    {column.items.map((item) => {
                                        const Icon = item.icon;

                                        return (
                                            <CommandCard
                                                key={item.id}
                                                title={item.title}
                                                description={item.description}
                                                icon={Icon ? <Icon width={16} height={16} /> : undefined}
                                                status={item.status}
                                                statusTone={item.statusTone}
                                                meta={item.meta}
                                                onClick={item.onClick}
                                            />
                                        );
                                    })}
                                </div>
                            ) : (
                                <Surface className={classes.emptyColumn} view="outlined" padding="m">
                                    {column.emptyLabel ?? "No commands"}
                                </Surface>
                            )}
                        </section>
                    ))}
                </div>

                {aside ? <aside className={classes.aside}>{aside}</aside> : null}
            </div>
        </section>
    );
}
