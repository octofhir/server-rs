import type { ReactNode } from "react";
import type { LabelProps } from "@gravity-ui/uikit";
import {
    Badge,
    Button,
    Text,
    AppLayout,
    type AppLayoutProps,
    type AppNavGroup,
    type AppNavItem,
} from "#/shared/ui";
import classes from "./AppShell.module.css";

export interface DashboardShellStatus {
    label: ReactNode;
    theme: LabelProps["theme"];
}

export interface DashboardShellAction {
    icon: ReactNode;
    label: string;
    onClick: () => void;
}

export interface DashboardShellAccount {
    name: ReactNode;
    signOutLabel?: string;
    onSignOut?: () => void;
}

export interface DashboardShellMenuItem extends AppNavItem {
    /** App-level alias for Gravity Navigation's `current` flag. */
    active?: boolean;
}

export interface DashboardShellMenuGroup extends AppNavGroup {
    /**
     * Convenience API for application shells. Gravity Navigation itself expects
     * flat menu items with `groupId`; DashboardShell accepts grouped items and
     * normalizes them before rendering.
     */
    items?: DashboardShellMenuItem[];
}

export interface DashboardShellProps
    extends Pick<
        AppLayoutProps,
        "logo" | "defaultPinned" | "persistKey" | "collapseBelow" | "children"
    > {
    menuItems?: DashboardShellMenuItem[];
    menuGroups?: DashboardShellMenuGroup[];
    themeAction?: DashboardShellAction;
    status?: DashboardShellStatus;
    account?: DashboardShellAccount | null;
}

/**
 * DashboardShell provides the high-level application structure.
 * Composes AppLayout with status indicators, theme toggles, and account management.
 */
export function DashboardShell({
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
}: DashboardShellProps) {
    const { normalizedMenuItems, normalizedMenuGroups } = normalizeNavigation(menuItems, menuGroups);

    return (
        <AppLayout
            logo={logo}
            menuItems={normalizedMenuItems}
            menuGroups={normalizedMenuGroups}
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
        </AppLayout>
    );
}

function normalizeNavigation(
    menuItems: DashboardShellMenuItem[] = [],
    menuGroups?: DashboardShellMenuGroup[],
): { normalizedMenuItems: AppNavItem[]; normalizedMenuGroups?: AppNavGroup[] } {
    if (!menuGroups?.length) {
        return {
            normalizedMenuItems: menuItems.map(normalizeMenuItem),
            normalizedMenuGroups: undefined,
        };
    }

    const groupedItems = menuGroups.flatMap((group) =>
        (group.items ?? []).map((item) =>
            normalizeMenuItem({
                ...item,
                groupId: item.groupId ?? group.id,
            }),
        ),
    );
    const normalizedMenuItems = [...menuItems.map(normalizeMenuItem), ...groupedItems];
    const usedGroupIds = new Set(
        normalizedMenuItems.flatMap((item) => (item.groupId ? [item.groupId] : [])),
    );
    const normalizedMenuGroups = menuGroups
        .filter((group) => usedGroupIds.has(group.id))
        .map(({ items: _items, ...group }) => group);

    return {
        normalizedMenuItems,
        normalizedMenuGroups: normalizedMenuGroups.length ? normalizedMenuGroups : undefined,
    };
}

function normalizeMenuItem({ active, current, ...item }: DashboardShellMenuItem): AppNavItem {
    return {
        ...item,
        current: current ?? active,
    };
}
