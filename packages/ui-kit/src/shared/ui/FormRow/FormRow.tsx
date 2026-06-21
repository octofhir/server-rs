import type { ReactNode } from "react";
import styles from "./FormRow.module.css";

export interface FormRowProps {
    label?: ReactNode;
    required?: boolean;
    /** Helper text shown under the control. */
    description?: ReactNode;
    /** Error message; replaces the description when present. */
    error?: ReactNode;
    /** Associates the label with a control. */
    htmlFor?: string;
    children?: ReactNode;
    className?: string;
}

export function FormRow({
    label,
    required,
    description,
    error,
    htmlFor,
    children,
    className,
}: FormRowProps) {
    return (
        <div className={[styles.row, className].filter(Boolean).join(" ")}>
            {label != null && (
                <label className={styles.label} htmlFor={htmlFor}>
                    {label}
                    {required && <span className={styles.required}>*</span>}
                </label>
            )}
            {children}
            {error != null ? (
                <span className={styles.error}>{error}</span>
            ) : (
                description != null && <span className={styles.description}>{description}</span>
            )}
        </div>
    );
}
