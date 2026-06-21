import { forwardRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { ActionIcon } from "../ActionIcon";
import styles from "./Alert.module.css";

export type AlertTheme = "info" | "success" | "warning" | "danger" | "neutral";

const colorToTheme: Record<string, AlertTheme> = {
    red: "danger",
    fire: "danger",
    green: "success",
    teal: "success",
    orange: "warning",
    yellow: "warning",
    warm: "warning",
    blue: "info",
    gray: "neutral",
    grey: "neutral",
};

export interface AlertProps extends Omit<React.HTMLAttributes<HTMLDivElement>, "title"> {
    theme?: AlertTheme;
    color?: string;
    variant?: string;
    title?: ReactNode;
    /** Body text. `children` also works and takes precedence. */
    message?: ReactNode;
    /** Leading icon. */
    icon?: ReactNode;
    /** Action buttons rendered under the message. */
    actions?: ReactNode;
    /** Show a close button; fires this handler. */
    onClose?: () => void;
    children?: ReactNode;
}

export const Alert = forwardRef<HTMLDivElement, AlertProps>(function Alert(
    {
        theme,
        color,
        variant: _variant,
        title,
        message,
        icon,
        actions,
        onClose,
        className,
        children,
        ...props
    },
    ref,
) {
    const effectiveTheme = theme ?? (color != null ? colorToTheme[color] : undefined) ?? "info";
    return (
        <div
            ref={ref}
            role="alert"
            className={[styles.alert, className].filter(Boolean).join(" ")}
            data-theme={effectiveTheme}
            {...props}
        >
            {icon != null && <span className={styles.icon}>{icon}</span>}
            <div className={styles.body}>
                {title != null && <div className={styles.title}>{title}</div>}
                {(children ?? message) != null && (
                    <div className={styles.message}>{children ?? message}</div>
                )}
                {actions != null && <div className={styles.actions}>{actions}</div>}
            </div>
            {onClose && (
                <ActionIcon
                    className={styles.close}
                    variant="subtle"
                    size="sm"
                    aria-label="Dismiss"
                    onClick={onClose}
                >
                    <X size={16} />
                </ActionIcon>
            )}
        </div>
    );
});
