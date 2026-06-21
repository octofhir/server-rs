import { forwardRef, type ReactNode } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { X } from "lucide-react";
import { ActionIcon } from "../ActionIcon";
import styles from "./Modal.module.css";

export type ModalSize = "xs" | "s" | "sm" | "m" | "md" | "l" | "lg" | "xl" | "auto";

const SIZE_WIDTH: Record<Exclude<ModalSize, "auto">, number> = {
    xs: 420,
    s: 420,
    sm: 480,
    m: 560,
    md: 560,
    l: 720,
    lg: 720,
    xl: 920,
};

export interface ModalProps {
    /** Controls visibility. */
    open?: boolean;
    /** Alias for `open`. */
    opened?: boolean;
    /** Fired on Escape / outside-click / close-button. */
    onClose?: () => void;
    /** Header title; when omitted, no header is rendered (unless a close button is shown). */
    title?: ReactNode;
    /** Max-width preset. `auto` lets content size itself. */
    size?: ModalSize;
    /** Show the header close (X) button. Defaults to `true` when a title is set. */
    withCloseButton?: boolean;
    /** Optional footer area (right-aligned actions). */
    footer?: ReactNode;
    /** Remove default body padding (e.g. for full-bleed content). */
    unpadded?: boolean;
    className?: string;
    bodyClassName?: string;
    children?: ReactNode;
}

/**
 * Dialog with surface chrome: header, optional close button and footer.
 * Control with `open` + `onClose`.
 */
export const Modal = forwardRef<HTMLDivElement, ModalProps>(function Modal(
    { open, opened, onClose, title, size = "m", withCloseButton, footer, unpadded, className, bodyClassName, children },
    ref,
) {
    const isOpen = open ?? opened ?? false;
    const showClose = withCloseButton ?? title != null;
    const maxWidth = size === "auto" ? undefined : SIZE_WIDTH[size];

    return (
        <Dialog.Root
            open={isOpen}
            onOpenChange={(next) => {
                if (!next) onClose?.();
            }}
        >
            <Dialog.Portal>
                <Dialog.Backdrop className={styles.backdrop} />
                <Dialog.Popup
                    ref={ref}
                    className={[styles.dialog, className].filter(Boolean).join(" ")}
                    style={maxWidth ? { maxWidth } : undefined}
                >
                    {(title != null || showClose) && (
                        <div className={styles.header}>
                            {title != null ? <Dialog.Title className={styles.title}>{title}</Dialog.Title> : <span />}
                            {showClose && (
                                <ActionIcon
                                    className={styles.close}
                                    view="flat"
                                    size="m"
                                    aria-label="Close dialog"
                                    onClick={() => onClose?.()}
                                >
                                    <X size={18} />
                                </ActionIcon>
                            )}
                        </div>
                    )}
                    <div
                        className={[styles.body, unpadded && styles.noPad, bodyClassName].filter(Boolean).join(" ")}
                    >
                        {children}
                    </div>
                    {footer != null && <div className={styles.footer}>{footer}</div>}
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
});
