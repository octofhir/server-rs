import {
    createElement,
    isValidElement,
    useCallback,
    useEffect,
    useState,
    type ComponentType,
    type ReactNode,
} from "react";
import { ChevronsLeft, ChevronsRight, LogOut, Moon, MoreVertical, Sun } from "lucide-react";
import { Button } from "../../shared/ui/Button";
import { Menu } from "../../shared/ui/Menu";
import { Modal } from "../../shared/ui/Modal";
import { Tooltip } from "../../shared/ui/Tooltip";
import styles from "./Sidebar.module.css";

type IconLike = ReactNode | ComponentType<{ size?: number }>;

function renderIcon(icon: IconLike | undefined, size = 18): ReactNode {
    if (icon == null) return null;
    if (isValidElement(icon)) return icon;
    // Component types: plain function components, or forwardRef/memo objects
    // (lucide icons are forwardRef, i.e. `{$$typeof, render}` objects).
    if (typeof icon === "function" || (typeof icon === "object" && "$$typeof" in icon)) {
        return createElement(icon as ComponentType<{ size?: number }>, { size });
    }
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
    colorScheme?: "light" | "dark";
    onToggleColorScheme?: () => void;
    collapsible?: boolean;
    collapsed?: boolean;
    defaultCollapsed?: boolean;
    onCollapsedChange?: (collapsed: boolean) => void;
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
    const [internalCollapsed, setInternalCollapsed] = useState(() => readPersisted(persistKey, defaultCollapsed));
    const collapsed = isControlled ? controlledCollapsed : internalCollapsed;
    const [signOutOpen, setSignOutOpen] = useState(false);

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
            "aria-label": collapsed && typeof item.label === "string" ? item.label : undefined,
        };

        const control =
            item.href && !item.disabled ? (
                <a {...shared} href={item.href} onClick={item.onClick}>
                    {inner}
                </a>
            ) : (
                <button {...shared} type="button" disabled={item.disabled} onClick={item.onClick}>
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

    const accountButton = account ? (
        <button type="button" className={styles.account} aria-label="Account menu">
            <span className={styles.avatar} aria-hidden="true">
                {initials(account.name)}
            </span>
            <span className={styles.accountText}>
                <span className={styles.accountName}>{account.name}</span>
                {account.secondary != null && <span className={styles.accountSecondary}>{account.secondary}</span>}
            </span>
            <MoreVertical size={16} className={styles.accountChevron} />
        </button>
    ) : null;

    return (
        <aside
            className={[styles.sidebar, className].filter(Boolean).join(" ")}
            data-collapsed={collapsed ? "true" : undefined}
        >
            {brand && (
                <div className={styles.brandRow}>
                    <button type="button" className={styles.brand} onClick={brand.onClick}>
                        <span className={styles.brandIcon}>
                            {brand.iconSrc ? <img src={brand.iconSrc} alt="" /> : renderIcon(brand.icon, 24)}
                        </span>
                        <span className={styles.brandText}>{brand.title}</span>
                    </button>
                </div>
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
                    <div className={styles.statusRow}>
                        <span className={styles.statusDot} data-theme={status.theme ?? "neutral"} />
                        <span className={styles.statusLabel}>{status.label}</span>
                    </div>
                )}

                {account && (
                    <Menu position="top-start">
                        <Menu.Target>{accountButton ?? <span />}</Menu.Target>
                        <Menu.Dropdown className={styles.accountMenu}>
                            {onToggleColorScheme && (
                                <Menu.Item
                                    leftSection={colorScheme === "dark" ? <Sun size={16} /> : <Moon size={16} />}
                                    onClick={onToggleColorScheme}
                                >
                                    {colorScheme === "dark" ? "Light mode" : "Dark mode"}
                                </Menu.Item>
                            )}
                            {account.onSignOut && (
                                <>
                                    <Menu.Divider />
                                    <Menu.Item
                                        color="danger"
                                        leftSection={<LogOut size={16} />}
                                        onClick={() => setSignOutOpen(true)}
                                    >
                                        Sign out
                                    </Menu.Item>
                                </>
                            )}
                        </Menu.Dropdown>
                    </Menu>
                )}

                {collapsible && (
                    <button
                        type="button"
                        className={styles.collapseToggle}
                        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
                        onClick={() => setCollapsed(!collapsed)}
                    >
                        <span className={styles.itemIcon}>
                            {collapsed ? <ChevronsRight size={18} /> : <ChevronsLeft size={18} />}
                        </span>
                        <span className={styles.itemLabel}>Collapse</span>
                    </button>
                )}
            </div>

            {account?.onSignOut && (
                <Modal
                    open={signOutOpen}
                    onClose={() => setSignOutOpen(false)}
                    title="Sign out"
                    size="xs"
                    footer={
                        <>
                            <Button view="flat" onClick={() => setSignOutOpen(false)}>
                                Cancel
                            </Button>
                            <Button
                                view="action-danger"
                                onClick={() => {
                                    setSignOutOpen(false);
                                    account.onSignOut?.();
                                }}
                            >
                                Sign out
                            </Button>
                        </>
                    }
                >
                    You will be signed out of the console.
                </Modal>
            )}
        </aside>
    );
}
