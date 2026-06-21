import type { ReactNode } from "react";
import { Button, SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./ControlPlaneWidgets.module.css";
import type { OperationCatalogItem } from "./types";
import { getAccessTone, getMethodClassName } from "./utils";

export interface OperationCatalogPanelProps {
    title?: ReactNode;
    description?: ReactNode;
    operations: OperationCatalogItem[];
    selectedOperationId?: string;
    emptyText?: string;
    onSelectOperation?: (operation: OperationCatalogItem) => void;
    onViewOperation?: (operation: OperationCatalogItem) => void;
}

function groupOperations(operations: OperationCatalogItem[]) {
    const groups = new Map<string, OperationCatalogItem[]>();

    for (const operation of operations) {
        const key = operation.category || "other";
        groups.set(key, [...(groups.get(key) ?? []), operation]);
    }

    return Array.from(groups.entries()).sort(([left], [right]) => left.localeCompare(right));
}

export function OperationCatalogPanel({
    title = "Operations",
    description,
    operations,
    selectedOperationId,
    emptyText = "No operations",
    onSelectOperation,
    onViewOperation,
}: OperationCatalogPanelProps) {
    const groups = groupOperations(operations);

    return (
        <SectionPanel
            title={title}
            description={description}
            actions={<StatusBadge tone="info">{operations.length.toLocaleString()} operations</StatusBadge>}
            view="outlined"
            padding="m"
        >
            {operations.length ? (
                <div className={classes.groupList}>
                    {groups.map(([category, categoryOperations]) => (
                        <div key={category} className={classes.group}>
                            <div className={classes.groupHeader}>
                                <div className={classes.groupTitle}>{category}</div>
                                <StatusBadge tone="neutral">{categoryOperations.length}</StatusBadge>
                            </div>
                            <div className={classes.list}>
                                {categoryOperations.map((operation) => {
                                    const Element = onSelectOperation ? "button" : "div";

                                    return (
                                        <Element
                                            key={operation.id}
                                            type={onSelectOperation ? "button" : undefined}
                                            className={[
                                                classes.operationItem,
                                                onSelectOperation ? classes.operationItemButton : undefined,
                                                operation.id === selectedOperationId ? classes.selected : undefined,
                                            ]
                                                .filter(Boolean)
                                                .join(" ")}
                                            onClick={
                                                onSelectOperation
                                                    ? () => onSelectOperation(operation)
                                                    : undefined
                                            }
                                        >
                                            <div className={classes.operationTop}>
                                                <div className={classes.titleBlock}>
                                                    <div className={classes.titleRow}>
                                                        <span className={classes.name}>{operation.name}</span>
                                                        <StatusBadge tone={getAccessTone(operation.public)}>
                                                            {operation.public ? "Public" : "Protected"}
                                                        </StatusBadge>
                                                        {operation.app ? (
                                                            <StatusBadge tone="info">{operation.app.name}</StatusBadge>
                                                        ) : null}
                                                    </div>
                                                    {operation.description ? (
                                                        <div className={classes.description}>
                                                            {operation.description}
                                                        </div>
                                                    ) : null}
                                                    <div className={classes.code}>{operation.pathPattern}</div>
                                                    <div className={classes.methodRow}>
                                                        {operation.methods.map((method) => (
                                                            <span key={method} className={getMethodClassName(method)}>
                                                                {method}
                                                            </span>
                                                        ))}
                                                    </div>
                                                </div>
                                                {onViewOperation ? (
                                                    <Button
                                                        size="sm"
                                                        view="flat-secondary"
                                                        onClick={(event) => {
                                                            event.stopPropagation();
                                                            onViewOperation(operation);
                                                        }}
                                                    >
                                                        Details
                                                    </Button>
                                                ) : null}
                                            </div>
                                        </Element>
                                    );
                                })}
                            </div>
                        </div>
                    ))}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
