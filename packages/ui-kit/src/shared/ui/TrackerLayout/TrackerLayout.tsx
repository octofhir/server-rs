import { forwardRef, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
    PageLayout,
    PageLayoutAside,
    type AsideHeaderProps,
    type MenuGroup as AsideMenuGroup,
    type MenuItem as AsideMenuItem,
} from "@gravity-ui/navigation";
import classes from "./TrackerLayout.module.css";

export type TrackerNavItem = AsideMenuItem;
export type TrackerNavGroup = AsideMenuGroup;

export interface TrackerLayoutProps
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

export const TrackerLayout = forwardRef<HTMLDivElement, TrackerLayoutProps>(
    function TrackerLayout(
        {
            defaultPinned = true,
            pinned,
            children,
            onChangePinned,
            menuGroups,
            persistKey,
            onMenuGroupsChanged,
            className,
            contentClassName,
            collapseBelow,
            topAlert,
            isCompactMode,
            ...rest
        },
        ref,
    ) {
        const persisted = useMemo(() => (persistKey ? readState(persistKey) : {}), [persistKey]);

        const [internalPinned, setInternalPinned] = useState<boolean>(
            persisted.pinned ?? defaultPinned,
        );
        const isControlled = pinned !== undefined;
        const shouldCollapseByViewport = useMediaQuery(
            collapseBelow ? `(max-width: ${collapseBelow}px)` : "(max-width: 0px)",
        );
        const resolvedPinned = isControlled ? (pinned as boolean) : internalPinned;
        const effectivePinned = shouldCollapseByViewport ? false : resolvedPinned;

        const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>(
            persisted.collapsedGroups ?? {},
        );

        const persist = useCallback(
            (next: PersistedState) => {
                if (!persistKey) return;
                writeState(persistKey, {
                    pinned: effectivePinned,
                    collapsedGroups,
                    ...next,
                });
            },
            [persistKey, effectivePinned, collapsedGroups],
        );

        useEffect(() => {
            if (persistKey) writeState(persistKey, { pinned: effectivePinned, collapsedGroups });
        }, [persistKey, effectivePinned, collapsedGroups]);

        const handlePinnedChange = (next: boolean) => {
            if (shouldCollapseByViewport) return;
            if (!isControlled) setInternalPinned(next);
            onChangePinned?.(next);
            persist({ pinned: next });
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
                className={className}
                pinned={effectivePinned}
                onChangePinned={handlePinnedChange}
                topAlert={topAlert}
                isCompactMode={isCompactMode}
            >
                <PageLayoutAside
                    ref={ref}
                    menuGroups={resolvedGroups}
                    onMenuGroupsChanged={handleGroupsChanged}
                    isCompactMode={isCompactMode}
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
