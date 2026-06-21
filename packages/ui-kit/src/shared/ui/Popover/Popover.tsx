import type { ReactNode } from "react";
import { Popover as BasePopover } from "@base-ui/react/popover";
import styles from "./Popover.module.css";

type Side = "top" | "bottom" | "left" | "right";
type Align = "start" | "center" | "end";

export type PopoverPlacement =
    | Side
    | `${Side}-start`
    | `${Side}-end`;

export interface PopoverProps {
    open?: boolean;
    defaultOpen?: boolean;
    onOpenChange?: (open: boolean) => void;
    /** Anchor placement, e.g. `bottom-end`. */
    placement?: PopoverPlacement;
    /** Reserved for API compatibility; only click is supported. */
    trigger?: "click";
    /** Floating panel contents. */
    content: ReactNode;
    /** Distance from the anchor, in px. */
    sideOffset?: number;
    /** Class applied to the floating panel. */
    className?: string;
    /** Trigger element. */
    children: ReactNode;
}

function parsePlacement(placement?: PopoverPlacement): { side: Side; align: Align } {
    if (!placement) return { side: "bottom", align: "center" };
    const [side, align] = placement.split("-") as [Side, Align | undefined];
    return { side, align: align ?? "center" };
}

export function Popover({
    open,
    defaultOpen,
    onOpenChange,
    placement,
    content,
    sideOffset = 6,
    className,
    children,
}: PopoverProps) {
    const { side, align } = parsePlacement(placement);
    return (
        <BasePopover.Root
            open={open}
            defaultOpen={defaultOpen}
            onOpenChange={(next) => onOpenChange?.(next)}
        >
            <BasePopover.Trigger nativeButton={false} render={<span className={styles.trigger} />}>
                {children}
            </BasePopover.Trigger>
            <BasePopover.Portal>
                <BasePopover.Positioner side={side} align={align} sideOffset={sideOffset}>
                    <BasePopover.Popup className={[styles.popup, className].filter(Boolean).join(" ")}>
                        {content}
                    </BasePopover.Popup>
                </BasePopover.Positioner>
            </BasePopover.Portal>
        </BasePopover.Root>
    );
}
