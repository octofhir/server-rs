import { createContext, isValidElement, useContext, type ReactElement, type ReactNode } from "react";
import { Menu as BaseMenu } from "@base-ui/react/menu";
import styles from "./Menu.module.css";

type Side = "top" | "right" | "bottom" | "left";
type Align = "start" | "center" | "end";
export type MenuPlacement = Side | `${Side}-start` | `${Side}-end`;

interface MenuPlacementValue {
    side: Side;
    align: Align;
}

const PlacementContext = createContext<MenuPlacementValue>({ side: "bottom", align: "end" });
const WidthContext = createContext<number | string | undefined>(undefined);

function parsePlacement(placement: MenuPlacement): MenuPlacementValue {
    const [side, sub] = placement.split("-") as [Side, "start" | "end" | undefined];
    return { side, align: sub ?? (placement.includes("-") ? "start" : "center") };
}

export interface MenuProps {
    children?: ReactNode;
    /** Popup placement relative to the trigger. */
    position?: MenuPlacement;
    /** Alias of {@link position}. */
    placement?: MenuPlacement;
    open?: boolean;
    defaultOpen?: boolean;
    onOpenChange?: (open: boolean) => void;
    /** Popup width / min-width. */
    width?: number | string;
    /** Accepted for compat; the kit menu uses its own shadow. */
    shadow?: string;
    /** Accepted for compat; the kit menu closes on item click by default. */
    closeOnItemClick?: boolean;
}

/**
 * Compound dropdown menu:
 * `<Menu><Menu.Target/><Menu.Dropdown><Menu.Item/></Menu.Dropdown></Menu>`.
 */
export function Menu({
    children,
    position,
    placement,
    open,
    defaultOpen,
    onOpenChange,
    width,
    shadow: _shadow,
    closeOnItemClick: _closeOnItemClick,
}: MenuProps) {
    const resolved = parsePlacement(position ?? placement ?? "bottom-end");
    return (
        <PlacementContext.Provider value={resolved}>
            <WidthContext.Provider value={width}>
                <BaseMenu.Root open={open} defaultOpen={defaultOpen} onOpenChange={onOpenChange}>
                    {children}
                </BaseMenu.Root>
            </WidthContext.Provider>
        </PlacementContext.Provider>
    );
}

export interface MenuTargetProps {
    children: ReactNode;
}

function MenuTarget({ children }: MenuTargetProps) {
    const trigger = isValidElement(children) ? (children as ReactElement) : <button type="button">{children}</button>;
    return <BaseMenu.Trigger render={trigger} />;
}

export interface MenuDropdownProps {
    children?: ReactNode;
    className?: string;
}

function MenuDropdown({ children, className }: MenuDropdownProps) {
    const { side, align } = useContext(PlacementContext);
    const width = useContext(WidthContext);
    return (
        <BaseMenu.Portal>
            <BaseMenu.Positioner side={side} align={align} sideOffset={6}>
                <BaseMenu.Popup
                    className={[styles.list, className].filter(Boolean).join(" ")}
                    style={width != null ? { minWidth: width } : undefined}
                >
                    {children}
                </BaseMenu.Popup>
            </BaseMenu.Positioner>
        </BaseMenu.Portal>
    );
}

export interface MenuItemProps {
    children?: ReactNode;
    leftSection?: ReactNode;
    rightSection?: ReactNode;
    onClick?: () => void;
    /** `"danger"` renders the destructive style. */
    color?: "danger" | "red" | "default";
    disabled?: boolean;
    /** Render as an anchor link. */
    component?: "a";
    href?: string;
    target?: string;
    rel?: string;
}

function MenuItem({
    children,
    leftSection,
    rightSection,
    onClick,
    color,
    disabled,
    component,
    href,
    target,
    rel,
}: MenuItemProps) {
    const danger = color === "danger" || color === "red";
    const className = [styles.item, danger && styles.danger].filter(Boolean).join(" ");
    const content = (
        <>
            {leftSection != null && <span className={styles.icon}>{leftSection}</span>}
            <span className={styles.label}>{children}</span>
            {rightSection != null && <span className={styles.icon}>{rightSection}</span>}
        </>
    );

    if (component === "a") {
        return (
            <BaseMenu.LinkItem
                className={className}
                href={href ?? "#"}
                target={target}
                rel={rel ?? (target === "_blank" ? "noopener noreferrer" : undefined)}
                aria-disabled={disabled || undefined}
            >
                {content}
            </BaseMenu.LinkItem>
        );
    }

    return (
        <BaseMenu.Item className={className} disabled={disabled} onClick={() => onClick?.()}>
            {content}
        </BaseMenu.Item>
    );
}

function MenuDivider() {
    return <BaseMenu.Separator className={styles.divider} />;
}

function MenuLabel({ children }: { children?: ReactNode }) {
    return <BaseMenu.GroupLabel className={styles.label}>{children}</BaseMenu.GroupLabel>;
}

Menu.Target = MenuTarget;
Menu.Dropdown = MenuDropdown;
Menu.Item = MenuItem;
Menu.Divider = MenuDivider;
Menu.Label = MenuLabel;
