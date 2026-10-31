import { ContextMenu } from "@base-ui/react/context-menu";
import { Fragment, type ReactNode } from "react";
import type { DropdownMenuItems } from "../Menu";
import menuStyles from "../Menu/Menu.module.css";
import classes from "./DataTable.module.css";

interface RowContextMenuProps {
    /** Items for the row that was right-clicked; null keeps the menu closed. */
    items: DropdownMenuItems | null;
    open: boolean;
    onOpenChange: (open: boolean) => void;
    children: ReactNode;
}

/** Right-click menu over the table body, styled like the kit's dropdown menu. */
export function RowContextMenu({ items, open, onOpenChange, children }: RowContextMenuProps) {
    const groups = (items ?? []).map((entry) => (Array.isArray(entry) ? entry : [entry]));
    return (
        <ContextMenu.Root open={open} onOpenChange={onOpenChange}>
            <ContextMenu.Trigger className={classes.menuArea}>{children}</ContextMenu.Trigger>
            <ContextMenu.Portal>
                <ContextMenu.Positioner>
                    <ContextMenu.Popup className={menuStyles.list}>
                        {groups.map((group, groupIndex) => (
                            // biome-ignore lint/suspicious/noArrayIndexKey: groups are positional
                            <Fragment key={groupIndex}>
                                {groupIndex > 0 && <ContextMenu.Separator className={menuStyles.divider} />}
                                {group.map((item, itemIndex) => (
                                    <ContextMenu.Item
                                        // biome-ignore lint/suspicious/noArrayIndexKey: items are positional
                                        key={itemIndex}
                                        className={[menuStyles.item, item.theme === "danger" && menuStyles.danger]
                                            .filter(Boolean)
                                            .join(" ")}
                                        disabled={item.disabled}
                                        onClick={() => item.action?.()}
                                    >
                                        {item.iconStart != null && <span className={menuStyles.icon}>{item.iconStart}</span>}
                                        <span className={menuStyles.label}>{item.text}</span>
                                    </ContextMenu.Item>
                                ))}
                            </Fragment>
                        ))}
                    </ContextMenu.Popup>
                </ContextMenu.Positioner>
            </ContextMenu.Portal>
        </ContextMenu.Root>
    );
}
