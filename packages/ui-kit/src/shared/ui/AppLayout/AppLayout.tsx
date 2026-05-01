import { forwardRef, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
    PageLayout,
    PageLayoutAside,
    type AsideHeaderProps,
    type MenuGroup as AsideMenuGroup,
    type MenuItem as AsideMenuItem,
} from "@gravity-ui/navigation";
import classes from "./AppLayout.module.css";

export type AppNavItem = AsideMenuItem;
export type AppNavGroup = AsideMenuGroup;

export interface AppLayoutProps
    extends Omit<AsideHeaderProps, "renderContent" | "pinned" | "menuGroups" | "className"> {
    defaultPinned?: boolean;
    pinned?: boolean;
    className?: string;
    contentClassName?: string;
    /** Collapse navigation below this viewport width. */
    collapseBelow?: number;
    /** Group definitions. Add `collapsible: true` to make a group collapsible. */
    menuGroups?: AsideMenuGroup[];
    /** Persist pinned + collapsed group state to localStorage under this key prefix. */
    persistKey?: string;
    children?: ReactNode;
    renderFooter?: (props: { isPinned: boolean }) => ReactNode;
}

interface PersistedState {
    pinned?: boolean;
    collapsedGroups?: Record<string, boolean>;
}

const readState = (key: string): PersistedState => {
    if (typeof window === "undefined") return {};
    try {
        const raw = window.localStorage.getItem(key);
        return raw ? (JSON.parse(raw) as PersistedState) : {};
    } catch {
        return {};
    }
};

const writeState = (key: string, value: PersistedState) => {
    if (typeof window === "undefined") return;
    try {
        window.localStorage.setItem(key, JSON.stringify(value));
    } catch {
        /* quota / disabled */
    }
};

function useMediaQuery(query: string) {
    const [matches, setMatches] = useState(() =>
        typeof window === "undefined" ? false : window.matchMedia(query).matches,
    );

    useEffect(() => {
        if (typeof window === "undefined") return undefined;

        const media = window.matchMedia(query);
        const handleChange = () => setMatches(media.matches);
        handleChange();
        media.addEventListener("change", handleChange);
        return () => media.removeEventListener("change", handleChange);
    }, [query]);

    return matches;
}

/**
 * Modern application layout with consistent sidebar, theme management,
 * and responsive behaviors.
 */
export const AppLayout = forwardRef<HTMLDivElement, AppLayoutProps>(
    function AppLayout(
        {
            defaultPinned = true,
            pinned: controlledPinned,
            children,
            onChangePinned,
            menuGroups,
            menuItems,
            persistKey,
            onMenuGroupsChanged,
            className,
            contentClassName,
            collapseBelow = 900,
            topAlert,
            isCompactMode,
            renderFooter,
            ...rest
        },
        ref,
    ) {
        const persisted = useMemo(() => (persistKey ? readState(persistKey) : {}), [persistKey]);

        const [internalPinned, setInternalPinned] = useState<boolean>(
            persisted.pinned ?? defaultPinned,
        );
        const isControlled = controlledPinned !== undefined;
        const shouldCollapseByViewport = useMediaQuery(`(max-width: ${collapseBelow}px)`);
        
        const resolvedPinned = isControlled ? (controlledPinned as boolean) : internalPinned;
        const effectivePinned = shouldCollapseByViewport ? false : resolvedPinned;

        const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>(
            persisted.collapsedGroups ?? {},
        );

        useEffect(() => {
            if (persistKey) {
                writeState(persistKey, { pinned: internalPinned, collapsedGroups });
            }
        }, [persistKey, internalPinned, collapsedGroups]);

        const handlePinnedChange = (next: boolean) => {
            if (shouldCollapseByViewport) return;
            if (!isControlled) setInternalPinned(next);
            onChangePinned?.(next);
        };

        const resolvedGroups = useMemo<AsideMenuGroup[] | undefined>(() => {
            if (!menuGroups) return undefined;
            return menuGroups.map((g) =>
                g.collapsible
                    ? {
                          ...g,
                          collapsed: collapsedGroups[g.id] ?? g.collapsedByDefault ?? false,
                      }
                    : g,
            );
        }, [menuGroups, collapsedGroups]);

        const visibleMenuItems = useMemo<AsideMenuItem[] | undefined>(() => {
            if (effectivePinned) return menuItems;

            return menuItems?.map(({ groupId: _groupId, ...item }) => item);
        }, [effectivePinned, menuItems]);

        const handleGroupsChanged = useCallback(
            (groups: AsideMenuGroup[]) => {
                const next: Record<string, boolean> = {};
                for (const g of groups) {
                    if (g.collapsible) next[g.id] = g.collapsed ?? g.collapsedByDefault ?? false;
                }
                setCollapsedGroups(next);
                onMenuGroupsChanged?.(groups);
            },
            [onMenuGroupsChanged],
        );

        return (
            <PageLayout
                className={[classes.layout, className].filter(Boolean).join(" ")}
                pinned={effectivePinned}
                onChangePinned={handlePinnedChange}
                topAlert={topAlert}
                isCompactMode={isCompactMode}
            >
                <PageLayoutAside
                    ref={ref}
                    menuItems={visibleMenuItems}
                    menuGroups={effectivePinned ? resolvedGroups : undefined}
                    onMenuGroupsChanged={handleGroupsChanged}
                    isCompactMode={isCompactMode}
                    renderFooter={renderFooter ? () => renderFooter({ isPinned: effectivePinned }) : undefined}
                    {...rest}
                />
                <PageLayout.Content>
                    <main className={[classes.content, contentClassName].filter(Boolean).join(" ")}>
                        {children}
                    </main>
                </PageLayout.Content>
            </PageLayout>
        );
    },
);
