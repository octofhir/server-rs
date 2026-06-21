import type { ReactNode } from "react";
import { Button, SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./ControlPlaneWidgets.module.css";
import type { AuthSessionSummary } from "./types";
import { getSessionTone } from "./utils";

export interface AuthSessionListPanelProps {
    title?: ReactNode;
    description?: ReactNode;
    sessions: AuthSessionSummary[];
    selectedSessionId?: string;
    emptyText?: string;
    revokeLabel?: string;
    revokingSessionId?: string;
    onSelectSession?: (session: AuthSessionSummary) => void;
    onRevokeSession?: (session: AuthSessionSummary) => void;
}

export function AuthSessionListPanel({
    title = "Sessions",
    description,
    sessions,
    selectedSessionId,
    emptyText = "No sessions",
    revokeLabel = "Revoke",
    revokingSessionId,
    onSelectSession,
    onRevokeSession,
}: AuthSessionListPanelProps) {
    return (
        <SectionPanel
            title={title}
            description={description}
            actions={<StatusBadge tone="info">{sessions.length.toLocaleString()} sessions</StatusBadge>}
            view="outlined"
            padding="m"
        >
            {sessions.length ? (
                <div className={classes.sessionList}>
                    {sessions.map((session) => {
                        const Element = onSelectSession ? "button" : "div";

                        return (
                            <Element
                                key={session.id}
                                type={onSelectSession ? "button" : undefined}
                                className={[
                                    classes.sessionItem,
                                    onSelectSession ? classes.sessionItemButton : undefined,
                                    session.id === selectedSessionId ? classes.selected : undefined,
                                    session.current ? classes.currentSession : undefined,
                                ]
                                    .filter(Boolean)
                                    .join(" ")}
                                onClick={onSelectSession ? () => onSelectSession(session) : undefined}
                            >
                                <div className={classes.sessionTop}>
                                    <div className={classes.sessionIdentity}>
                                        <div className={classes.titleRow}>
                                            <span className={classes.sessionDevice}>{session.deviceName}</span>
                                            {session.current ? (
                                                <StatusBadge tone="success">Current</StatusBadge>
                                            ) : null}
                                            <StatusBadge tone={getSessionTone(session.status)}>
                                                {session.status ?? "unknown"}
                                            </StatusBadge>
                                        </div>
                                        {session.browserName ? (
                                            <div className={classes.description}>{session.browserName}</div>
                                        ) : null}
                                    </div>
                                    {onRevokeSession && !session.current ? (
                                        <Button
                                            size="sm"
                                            variant="subtle" color="red"
                                            loading={revokingSessionId === session.id}
                                            onClick={(event) => {
                                                event.stopPropagation();
                                                onRevokeSession(session);
                                            }}
                                        >
                                            {revokeLabel}
                                        </Button>
                                    ) : null}
                                </div>
                                <div className={classes.sessionMetaGrid}>
                                    <div className={classes.sessionMetaCell}>
                                        <div className={classes.sessionMetaLabel}>IP address</div>
                                        <div className={classes.sessionMetaValue}>
                                            {session.ipAddress ?? "Unknown"}
                                        </div>
                                    </div>
                                    <div className={classes.sessionMetaCell}>
                                        <div className={classes.sessionMetaLabel}>Last active</div>
                                        <div className={classes.sessionMetaValue}>
                                            {session.lastActivityLabel ?? "Unknown"}
                                        </div>
                                    </div>
                                    <div className={classes.sessionMetaCell}>
                                        <div className={classes.sessionMetaLabel}>Expires</div>
                                        <div className={classes.sessionMetaValue}>
                                            {session.expiresLabel ?? "Unknown"}
                                        </div>
                                    </div>
                                </div>
                            </Element>
                        );
                    })}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
