import { forwardRef, type ReactNode } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { X } from "lucide-react";
import { ActionIcon } from "../ActionIcon";
import styles from "./Drawer.module.css";

export type DrawerPlacement = "left" | "right" | "top" | "bottom";

export interface DrawerProps {
    open?: boolean;
    /** Alias for `open`. */
    opened?: boolean;
    onOpenChange?: (open: boolean) => void;
    /** Fired on Escape / outside-click / close button. */
    onClose?: () => void;
    /** Edge the panel slides in from. */
    placement?: DrawerPlacement;
    /** Width (left/right) or height (top/bottom). Number is treated as px. */
    size?: number | string;
    title?: ReactNode;
    /** Show the header close (X) button. Defaults to `true` when a title is set. */
    withCloseButton?: boolean;
    /** Remove default body padding. */
    unpadded?: boolean;
    className?: string;
    children?: ReactNode;
}

export const Drawer = forwardRef<HTMLDivElement, DrawerProps>(function Drawer(
    {
        open,
        opened,
        onOpenChange,
        onClose,
        placement = "right",
        size = 360,
        title,
        withCloseButton,
        unpadded,
        className,
        children,
    },
    ref,
) {
    const isOpen = open ?? opened ?? false;
    const showClose = withCloseButton ?? title != null;
    const sizeValue = typeof size === "number" ? `${size}px` : size;
    const close = () => {
        onOpenChange?.(false);
        onClose?.();
    };

    return (
        <Dialog.Root
            open={isOpen}
            onOpenChange={(next) => {
                onOpenChange?.(next);
                if (!next) onClose?.();
            }}
        >
            <Dialog.Portal>
                <Dialog.Backdrop className={styles.backdrop} />
                <Dialog.Popup
                    ref={ref}
                    data-placement={placement}
                    className={[styles.popup, className].filter(Boolean).join(" ")}
                    style={{ "--drawer-size": sizeValue } as React.CSSProperties}
                >
                    {(title != null || showClose) && (
                        <div className={styles.header}>
                            {title != null ? (
                                <Dialog.Title className={styles.title}>{title}</Dialog.Title>
                            ) : (
                                <span />
                            )}
                            {showClose && (
                                <ActionIcon view="flat" size="m" aria-label="Close drawer" onClick={close}>
                                    <X size={18} />
                                </ActionIcon>
                            )}
                        </div>
                    )}
                    <div className={[styles.body, unpadded && styles.noPad].filter(Boolean).join(" ")}>
                        {children}
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
});
