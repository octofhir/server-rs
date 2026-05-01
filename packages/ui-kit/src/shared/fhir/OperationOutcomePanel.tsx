import type { ReactNode } from "react";
import { StatusBadge, Surface, type StatusTone } from "../ui";
import classes from "./OperationOutcomePanel.module.css";

export type OperationOutcomeSeverity = "fatal" | "error" | "warning" | "information";

export interface OperationOutcomeIssue {
    severity?: OperationOutcomeSeverity;
    code?: string;
    diagnostics?: string;
    location?: string[];
    expression?: string[];
}

export interface OperationOutcomeLike {
    resourceType?: "OperationOutcome";
    issue?: OperationOutcomeIssue[];
}

export interface OperationOutcomePanelProps {
    outcome?: OperationOutcomeLike | null;
    issues?: OperationOutcomeIssue[];
    title?: ReactNode;
    emptyText?: ReactNode;
    maxIssues?: number;
    className?: string;
}

const toneBySeverity: Record<OperationOutcomeSeverity, StatusTone> = {
    fatal: "danger",
    error: "danger",
    warning: "warning",
    information: "info",
};

function getIssuePath(issue: OperationOutcomeIssue) {
    const paths = issue.expression?.length ? issue.expression : issue.location;
    return paths?.join(", ");
}

export function OperationOutcomePanel({
    outcome,
    issues,
    title = "OperationOutcome",
    emptyText = "No issues reported.",
    maxIssues,
    className,
}: OperationOutcomePanelProps) {
    const allIssues = issues ?? outcome?.issue ?? [];
    const visibleIssues = maxIssues ? allIssues.slice(0, maxIssues) : allIssues;
    const remainingCount = Math.max(allIssues.length - visibleIssues.length, 0);

    return (
        <Surface className={[classes.root, className].filter(Boolean).join(" ")} view="tinted" padding="s">
            <div className={classes.header}>
                <div className={classes.title}>{title}</div>
                <StatusBadge tone={allIssues.length ? "danger" : "neutral"}>
                    {allIssues.length} issue{allIssues.length === 1 ? "" : "s"}
                </StatusBadge>
            </div>

            {visibleIssues.length ? (
                <div className={classes.list}>
                    {visibleIssues.map((issue, index) => {
                        const path = getIssuePath(issue);

                        return (
                            <div className={classes.issue} key={`${issue.severity}-${issue.code}-${index}`}>
                                <div className={classes.issueHead}>
                                    <StatusBadge tone={issue.severity ? toneBySeverity[issue.severity] : "neutral"}>
                                        {issue.severity ?? "unknown"}
                                    </StatusBadge>
                                    <div className={classes.code}>{issue.code ?? "unknown"}</div>
                                </div>
                                {issue.diagnostics ? (
                                    <div className={classes.diagnostics}>{issue.diagnostics}</div>
                                ) : null}
                                {path ? <div className={classes.path}>{path}</div> : null}
                            </div>
                        );
                    })}
                    {remainingCount ? (
                        <div className={classes.empty}>+{remainingCount} more issues</div>
                    ) : null}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </Surface>
    );
}
