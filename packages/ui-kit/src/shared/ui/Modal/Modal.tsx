import { forwardRef, type ReactNode } from "react";
import { Modal as GravityModal, type ModalProps as GravityModalProps } from "@gravity-ui/uikit";
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

type ModalPassthrough = Pick<
    GravityModalProps,
    "container" | "disablePortal" | "keepMounted" | "initialFocus" | "returnFocus" | "contentClassName"
>;

export interface ModalProps extends ModalPassthrough {
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
    {
        open,
        opened,
        onClose,
        title,
        size = "m",
        withCloseButton,
        footer,
        unpadded,
        className,
        bodyClassName,
        children,
        ...passthrough
    },
    ref,
) {
    const isOpen = open ?? opened ?? false;
    const showClose = withCloseButton ?? title != null;
    const maxWidth = size === "auto" ? undefined : SIZE_WIDTH[size];

    return (
        <GravityModal
            open={isOpen}
            onOpenChange={(next) => {
                if (!next) onClose?.();
            }}
            {...passthrough}
        >
            <div
                ref={ref}
                className={[styles.dialog, className].filter(Boolean).join(" ")}
                style={maxWidth ? { maxWidth } : undefined}
            >
                {(title != null || showClose) && (
                    <div className={styles.header}>
                        {title != null ? <h2 className={styles.title}>{title}</h2> : <span />}
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
                    className={[styles.body, unpadded && styles.noPad, bodyClassName]
                        .filter(Boolean)
                        .join(" ")}
                >
                    {children}
                </div>
                {footer != null && <div className={styles.footer}>{footer}</div>}
            </div>
        </GravityModal>
    );
});
