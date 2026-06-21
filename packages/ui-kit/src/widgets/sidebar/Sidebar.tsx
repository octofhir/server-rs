import {
    createElement,
    isValidElement,
    useCallback,
    useEffect,
    useState,
    type ComponentType,
    type ReactNode,
} from "react";
import { ChevronsLeft, ChevronsRight, LogOut, Moon, Sun } from "lucide-react";
import { ActionIcon } from "../../shared/ui/ActionIcon";
import { Tooltip } from "../../shared/ui/Tooltip";
import styles from "./Sidebar.module.css";

type IconLike = ReactNode | ComponentType<{ size?: number }>;

function renderIcon(icon: IconLike | undefined, size = 18): ReactNode {
    if (icon == null) return null;
    if (isValidElement(icon)) return icon;
    if (typeof icon === "function") return createElement(icon as ComponentType<{ size?: number }>, { size });
    return icon;
}

export interface SidebarNavItem {
    id: string;
    label: ReactNode;
    icon?: IconLike;
    href?: string;
    active?: boolean;
    onClick?: () => void;
    badge?: ReactNode;
    disabled?: boolean;
}

export interface SidebarNavGroup {
    id: string;
    label?: ReactNode;
    items: SidebarNavItem[];
}

export interface SidebarBrand {
    title: ReactNode;
    icon?: IconLike;
    iconSrc?: string;
    href?: string;
    onClick?: () => void;
}

export interface SidebarAccount {
    name: ReactNode;
    secondary?: ReactNode;
    onSignOut?: () => void;
}

export interface SidebarStatus {
    label: ReactNode;
    theme?: "success" | "warning" | "danger" | "neutral";
}

export interface SidebarProps {
    brand?: SidebarBrand;
    groups: SidebarNavGroup[];
    account?: SidebarAccount | null;
    status?: SidebarStatus;
    /** Light/dark toggle in the footer. */
    colorScheme?: "light" | "dark";
    onToggleColorScheme?: () => void;
    collapsible?: boolean;
    collapsed?: boolean;
    defaultCollapsed?: boolean;
    onCollapsedChange?: (collapsed: boolean) => void;
    /** Persist the collapsed state to localStorage under this key. */
    persistKey?: string;
    className?: string;
}

function readPersisted(key: string | undefined, fallback: boolean): boolean {
    if (!key || typeof window === "undefined") return fallback;
    try {
        const v = window.localStorage.getItem(key);
        return v == null ? fallback : v === "1";
    } catch {
        return fallback;
    }
}

function initials(name: ReactNode): string {
    if (typeof name !== "string") return "?";
    return name
        .split(/\s+/)
        .filter(Boolean)
        .slice(0, 2)
        .map((w) => w[0]?.toUpperCase() ?? "")
        .join("");
}

export function Sidebar({
    brand,
    groups,
    account,
    status,
    colorScheme,
    onToggleColorScheme,
    collapsible = true,
    collapsed: controlledCollapsed,
    defaultCollapsed = false,
    onCollapsedChange,
    persistKey,
    className,
}: SidebarProps) {
    const isControlled = controlledCollapsed != null;
    const [internalCollapsed, setInternalCollapsed] = useState(() =>
        readPersisted(persistKey, defaultCollapsed),
    );
    const collapsed = isControlled ? controlledCollapsed : internalCollapsed;

    const setCollapsed = useCallback(
        (next: boolean) => {
            if (!isControlled) setInternalCollapsed(next);
            onCollapsedChange?.(next);
        },
        [isControlled, onCollapsedChange],
    );

    useEffect(() => {
        if (!persistKey || typeof window === "undefined") return;
        try {
            window.localStorage.setItem(persistKey, collapsed ? "1" : "0");
        } catch {
            /* ignore */
        }
    }, [persistKey, collapsed]);

    const renderItem = (item: SidebarNavItem) => {
        const inner = (
            <>
                <span className={styles.itemIcon}>{renderIcon(item.icon, 18)}</span>
                <span className={styles.itemLabel}>{item.label}</span>
                {item.badge != null && <span className={styles.itemBadge}>{item.badge}</span>}
            </>
        );
        const shared = {
            className: styles.item,
            "data-active": item.active ? "true" : undefined,
            "data-disabled": item.disabled ? "true" : undefined,
            "aria-current": item.active ? ("page" as const) : undefined,
        };

        const control =
            item.href && !item.disabled ? (
                <a
                    {...shared}
                    href={item.href}
                    onClick={item.onClick}
                    aria-label={collapsed && typeof item.label === "string" ? item.label : undefined}
                >
                    {inner}
                </a>
            ) : (
                <button
                    {...shared}
                    type="button"
                    disabled={item.disabled}
                    onClick={item.onClick}
                    aria-label={collapsed && typeof item.label === "string" ? item.label : undefined}
                >
                    {inner}
                </button>
            );

        if (collapsed && typeof item.label === "string") {
            return (
                <Tooltip key={item.id} content={item.label} placement="right">
                    {control}
                </Tooltip>
            );
        }
        return <div key={item.id}>{control}</div>;
    };

    return (
        <aside className={[styles.sidebar, className].filter(Boolean).join(" ")} data-collapsed={collapsed ? "true" : undefined}>
            {brand && (
                <button type="button" className={styles.brand} onClick={brand.onClick}>
                    <span className={styles.brandIcon}>
                        {brand.iconSrc ? <img src={brand.iconSrc} alt="" /> : renderIcon(brand.icon, 24)}
                    </span>
                    <span className={styles.brandText}>{brand.title}</span>
                </button>
            )}

            <nav className={styles.nav} aria-label="Main navigation">
                {groups.map((group) => (
                    <div key={group.id} className={styles.group}>
                        {group.label != null && !collapsed && <div className={styles.groupLabel}>{group.label}</div>}
                        {group.items.map(renderItem)}
                    </div>
                ))}
            </nav>

            <div className={styles.footer}>
                {status && (
                    <div className={styles.footerRow}>
                        <span className={styles.status}>
                            <span className={styles.statusDot} data-theme={status.theme ?? "neutral"} />
                            <span className={styles.statusLabel}>{status.label}</span>
                        </span>
                        <span className={styles.spacer} />
                        {onToggleColorScheme && (
                            <ActionIcon
                                view="flat"
                                size="s"
                                aria-label="Toggle color scheme"
                                onClick={onToggleColorScheme}
                            >
                                {colorScheme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
                            </ActionIcon>
                        )}
                    </div>
                )}

                {account && (
                    <div className={styles.footerRow}>
                        <span className={styles.account}>
                            <span className={styles.avatar} aria-hidden="true">
                                {initials(account.name)}
                            </span>
                            <span className={styles.accountText}>
                                <span className={styles.accountName}>{account.name}</span>
                                {account.secondary != null && (
                                    <span className={styles.accountSecondary}>{account.secondary}</span>
                                )}
                            </span>
                        </span>
                        <span className={styles.spacer} />
                        {account.onSignOut && (
                            <ActionIcon view="flat" size="s" aria-label="Sign out" onClick={account.onSignOut}>
                                <LogOut size={16} />
                            </ActionIcon>
                        )}
                    </div>
                )}

                {collapsible && (
                    <div className={styles.footerRow}>
                        <span className={styles.spacer} />
                        <ActionIcon
                            view="flat"
                            size="s"
                            aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
                            onClick={() => setCollapsed(!collapsed)}
                        >
                            {collapsed ? <ChevronsRight size={16} /> : <ChevronsLeft size={16} />}
                        </ActionIcon>
                    </div>
                )}
            </div>
        </aside>
    );
}
