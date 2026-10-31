import { ContextMenu } from "@base-ui/react/context-menu";
import type { ReactNode } from "react";
import type { DropdownMenuItems } from "../Menu";
import { ContextMenuItems } from "../Menu/ContextMenu";
import classes from "./DataTable.module.css";

interface RowContextMenuProps {
    /** Items for the row that was right-clicked; null keeps the menu closed. */
    items: DropdownMenuItems | null;
    open: boolean;
    onOpenChange: (open: boolean) => void;
    children: ReactNode;
}

/** One right-click menu over the whole table body; the row decides the items. */
export function RowContextMenu({ items, open, onOpenChange, children }: RowContextMenuProps) {
    return (
        <ContextMenu.Root open={open} onOpenChange={onOpenChange}>
            <ContextMenu.Trigger className={classes.menuArea}>{children}</ContextMenu.Trigger>
            {items && <ContextMenuItems items={items} />}
        </ContextMenu.Root>
    );
}
