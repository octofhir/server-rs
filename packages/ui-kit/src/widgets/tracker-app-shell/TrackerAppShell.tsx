import type { ReactNode } from "react";
import type { LabelProps } from "@gravity-ui/uikit";
import {
    Badge,
    Button,
    Text,
    TrackerLayout,
    type TrackerLayoutProps,
    type TrackerNavGroup,
    type TrackerNavItem,
} from "#/shared/ui";
import classes from "./TrackerAppShell.module.css";

export interface TrackerAppShellStatus {
    label: ReactNode;
    theme: LabelProps["theme"];
}

export interface TrackerAppShellAction {
    icon: ReactNode;
    label: string;
    onClick: () => void;
}

export interface TrackerAppShellAccount {
    name: ReactNode;
    signOutLabel?: string;
    onSignOut?: () => void;
}

export interface TrackerAppShellProps
    extends Pick<
        TrackerLayoutProps,
        "logo" | "defaultPinned" | "persistKey" | "collapseBelow" | "children"
    > {
    menuItems: TrackerNavItem[];
    menuGroups?: TrackerNavGroup[];
    themeAction?: TrackerAppShellAction;
    status?: TrackerAppShellStatus;
    account?: TrackerAppShellAccount | null;
}

export function TrackerAppShell({
    logo,
    menuItems,
    menuGroups,
    defaultPinned = true,
    persistKey,
    collapseBelow = 900,
    themeAction,
    status,
    account,
    children,
}: TrackerAppShellProps) {
    return (
        <TrackerLayout
            logo={logo}
            menuItems={menuItems}
            menuGroups={menuGroups}
            defaultPinned={defaultPinned}
            persistKey={persistKey}
            collapseBelow={collapseBelow}
            contentClassName={classes.content}
            renderFooter={({ isPinned }) => (
                <div className={classes.footer}>
                    {themeAction ? (
                        <div
                            className={[
                                classes.footerRow,
                                isPinned ? classes.footerRowPinned : classes.footerRowCollapsed,
                            ].join(" ")}
                        >
                            <Button
                                className={classes.themeButton}
                                view="flat-secondary"
                                size="m"
                                onClick={themeAction.onClick}
                                aria-label={themeAction.label}
                                title={themeAction.label}
                            >
                                <Button.Icon>{themeAction.icon}</Button.Icon>
                                {isPinned ? (
                                    <span className={classes.themeLabel}>{themeAction.label}</span>
                                ) : null}
                            </Button>
                            {isPinned && status ? (
                                <Badge theme={status.theme} size="s">
                                    {status.label}
                                </Badge>
                            ) : null}
                        </div>
                    ) : null}

                    {isPinned && account ? (
                        <div className={classes.account}>
                            <Text
                                as="span"
                                variant="body-1"
                                color="secondary"
                                className={classes.accountName}
                            >
                                {account.name}
                            </Text>
                            {account.onSignOut ? (
                                <Button view="flat-secondary" size="s" onClick={account.onSignOut}>
                                    {account.signOutLabel ?? "Sign out"}
                                </Button>
                            ) : null}
                        </div>
                    ) : null}
                </div>
            )}
        >
            {children}
        </TrackerLayout>
    );
}
