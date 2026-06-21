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

export interface DropdownMenuProps {
    items: DropdownMenuItems;
    /** Trigger icon (defaults to a vertical ellipsis). */
    icon?: ReactNode;
    size?: Size;
    /** Props forwarded to the default icon-button trigger. */
    defaultSwitcherProps?: {
        view?: ButtonProps["view"];
        size?: Size;
        "aria-label"?: string;
    };
    popupProps?: { placement?: MenuPlacement };
    /** Custom trigger element; overrides the default icon button. */
    switcher?: ReactNode;
}

export function DropdownMenu({
    items,
    icon,
    size,
    defaultSwitcherProps,
    popupProps,
    switcher,
}: DropdownMenuProps) {
    const groups = items.map((entry) => (Array.isArray(entry) ? entry : [entry]));
    const trigger = switcher ?? (
        <ActionIcon
            view={defaultSwitcherProps?.view ?? "flat"}
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
