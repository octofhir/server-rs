import type { ReactNode } from "react";
import { KeyValueList, SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./ControlPlaneWidgets.module.css";
import type { OperationCatalogItem } from "./types";
import { getAccessTone, getMethodClassName } from "./utils";

export interface OperationDetailPanelProps {
    title?: ReactNode;
    description?: ReactNode;
    operation?: OperationCatalogItem;
    emptyText?: string;
}

export function OperationDetailPanel({
    title = "Operation detail",
    description,
    operation,
    emptyText = "Select an operation to inspect its route contract.",
}: OperationDetailPanelProps) {
    return (
        <SectionPanel title={title} description={description} view="outlined" padding="m">
            {operation ? (
                <div className={classes.detailStack}>
                    <div className={classes.detailHeader}>
                        <div className={classes.titleBlock}>
                            <div className={classes.titleRow}>
                                <span className={classes.name}>{operation.name}</span>
                                <StatusBadge tone={getAccessTone(operation.public)}>
                                    {operation.public ? "Public" : "Protected"}
                                </StatusBadge>
                            </div>
                            <div className={classes.code}>{operation.id}</div>
                        </div>
                        <div className={classes.methodRow}>
                            {operation.methods.map((method) => (
                                <span key={method} className={getMethodClassName(method)}>
                                    {method}
                                </span>
                            ))}
                        </div>
                    </div>
                    {operation.description ? (
                        <div className={classes.description}>{operation.description}</div>
                    ) : null}
                    <div className={classes.detailBlock}>
                        <KeyValueList
                            items={[
                                { id: "category", label: "Category", value: operation.category },
                                { id: "path", label: "Path", value: operation.pathPattern },
                                { id: "module", label: "Module", value: operation.module ?? "-" },
                                { id: "app", label: "App", value: operation.app?.name ?? "-" },
                            ]}
                        />
                    </div>
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
