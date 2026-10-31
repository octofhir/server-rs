import { ContextMenu as BaseContextMenu } from "@base-ui/react/context-menu";
import { Fragment, type ReactElement } from "react";
import { type DropdownMenuItems, groupMenuItems } from "./DropdownMenu";
import styles from "./Menu.module.css";

/** Menu entries rendered inside a Base UI context-menu popup. */
export function ContextMenuItems({ items }: { items: DropdownMenuItems }) {
    const groups = groupMenuItems(items);
    return (
        <BaseContextMenu.Portal>
            <BaseContextMenu.Positioner>
                <BaseContextMenu.Popup className={styles.list}>
                    {groups.map((group, groupIndex) => (
                        // biome-ignore lint/suspicious/noArrayIndexKey: groups are positional
                        <Fragment key={groupIndex}>
                            {groupIndex > 0 && <BaseContextMenu.Separator className={styles.divider} />}
                            {group.map((item, itemIndex) => (
                                <BaseContextMenu.Item
                                    // biome-ignore lint/suspicious/noArrayIndexKey: items are positional
                                    key={itemIndex}
                                    className={[styles.item, item.theme === "danger" && styles.danger]
                                        .filter(Boolean)
                                        .join(" ")}
                                    disabled={item.disabled}
                                    onClick={() => item.action?.()}
                                >
                                    {item.iconStart != null && <span className={styles.icon}>{item.iconStart}</span>}
                                    <span className={styles.label}>{item.text}</span>
                                </BaseContextMenu.Item>
                            ))}
                        </Fragment>
                    ))}
                </BaseContextMenu.Popup>
            </BaseContextMenu.Positioner>
        </BaseContextMenu.Portal>
    );
}

export interface ContextMenuProps {
    items: DropdownMenuItems;
    /** The element that opens the menu on right-click; it must forward its ref and props (e.g. `Table.Tr`). */
    children: ReactElement;
}

/**
 * Right-click menu on any element, styled like `DropdownMenu`. The trigger gets
 * `data-popup-open` while the menu is open, so a row can stay highlighted.
 */
export function ContextMenu({ items, children }: ContextMenuProps) {
    return (
        <BaseContextMenu.Root>
            <BaseContextMenu.Trigger render={children} />
            <ContextMenuItems items={items} />
        </BaseContextMenu.Root>
    );
}
