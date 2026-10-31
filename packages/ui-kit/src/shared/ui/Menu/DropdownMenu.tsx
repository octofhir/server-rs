import { Fragment, type ReactNode } from "react";
import { EllipsisVertical } from "lucide-react";
import { ActionIcon } from "../ActionIcon";
import type { ButtonProps } from "../Button";
import type { Size } from "../layout-utils";
import { Menu, type MenuPlacement } from "./Menu";

export interface DropdownMenuItem {
    text: ReactNode;
    iconStart?: ReactNode;
    action?: () => void;
    theme?: "danger" | "normal" | "default";
    disabled?: boolean;
    href?: string;
}

/** Items, optionally grouped — a nested array renders as a divided section. */
export type DropdownMenuItems = (DropdownMenuItem | DropdownMenuItem[])[];

/** Consecutive plain items share a section; each nested array is a section of its own. */
export function groupMenuItems(items: DropdownMenuItems): DropdownMenuItem[][] {
    const groups: DropdownMenuItem[][] = [];
    let loose: DropdownMenuItem[] | null = null;
    for (const entry of items) {
        if (Array.isArray(entry)) {
            groups.push(entry);
            loose = null;
        } else {
            if (!loose) {
                loose = [];
                groups.push(loose);
            }
            loose.push(entry);
        }
    }
    return groups.filter((group) => group.length > 0);
}

export interface DropdownMenuProps {
    items: DropdownMenuItems;
    /** Trigger icon (defaults to a vertical ellipsis). */
    icon?: ReactNode;
    size?: Size;
    /** Props forwarded to the default icon-button trigger. */
    defaultSwitcherProps?: {
        variant?: ButtonProps["variant"];
        size?: Size;
        "aria-label"?: string;
    };
    popupProps?: { placement?: MenuPlacement };
    /** Custom trigger element; overrides the default icon button. */
    switcher?: ReactNode;
    /** Render the trigger from a function; overrides `switcher`. */
    renderSwitcher?: (props: Record<string, never>) => ReactNode;
}

export function DropdownMenu({
    items,
    icon,
    size,
    defaultSwitcherProps,
    popupProps,
    switcher,
    renderSwitcher,
}: DropdownMenuProps) {
    const groups = groupMenuItems(items);
    const trigger = renderSwitcher?.({}) ?? switcher ?? (
        <ActionIcon
            variant={defaultSwitcherProps?.variant ?? "subtle"}
            size={size ?? defaultSwitcherProps?.size ?? "sm"}
            aria-label={defaultSwitcherProps?.["aria-label"] ?? "Open menu"}
        >
            {icon ?? <EllipsisVertical size={16} />}
        </ActionIcon>
    );

    return (
        <Menu placement={popupProps?.placement ?? "bottom-end"}>
            <Menu.Target>{trigger}</Menu.Target>
            <Menu.Dropdown>
                {groups.map((group, groupIndex) => (
                    // biome-ignore lint/suspicious/noArrayIndexKey: groups are positional
                    <Fragment key={groupIndex}>
                        {groupIndex > 0 && <Menu.Divider />}
                        {group.map((item, itemIndex) => (
                            <Menu.Item
                                // biome-ignore lint/suspicious/noArrayIndexKey: items are positional
                                key={itemIndex}
                                leftSection={item.iconStart}
                                color={item.theme === "danger" ? "danger" : "default"}
                                disabled={item.disabled}
                                onClick={item.action}
                                component={item.href ? "a" : undefined}
                                href={item.href}
                            >
                                {item.text}
                            </Menu.Item>
                        ))}
                    </Fragment>
                ))}
            </Menu.Dropdown>
        </Menu>
    );
}
