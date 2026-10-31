import { X } from "lucide-react";
import { type ReactNode, useEffect } from "react";
import { Portal } from "../Portal";
import styles from "./SelectionBar.module.css";

export interface SelectionBarAction {
    label: string;
    icon?: ReactNode;
    onClick: () => void;
    danger?: boolean;
    disabled?: boolean;
}

export interface SelectionBarProps {
    /** Number of selected items; the bar is hidden at 0. */
    count: number;
    actions: SelectionBarAction[];
    /** Clears the selection (also bound to Esc while the bar is shown). */
    onClear: () => void;
    /** Noun for the count, e.g. "patients"; defaults to "selected" alone. */
    noun?: string;
}

/** Floating bar for bulk actions on a selection, docked at the bottom centre of the viewport. */
export function SelectionBar({ count, actions, onClear, noun }: SelectionBarProps) {
    useEffect(() => {
        if (count === 0) return;
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key !== "Escape" || event.defaultPrevented) return;
            if (document.querySelector("[role='dialog'], [role='menu']")) return;
            onClear();
        };
        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, [count, onClear]);

    if (count === 0) return null;
    return (
        <Portal>
            <div className={styles.bar} role="toolbar" aria-label="Selection actions">
                <span className={styles.count}>
                    {count} {noun ? `${noun} ` : ""}selected
                </span>
                <span className={styles.divider} />
                {actions.map((action) => (
                    <button
                        key={action.label}
                        type="button"
                        className={styles.action}
                        data-danger={action.danger ? "true" : undefined}
                        disabled={action.disabled}
                        onClick={action.onClick}
                    >
                        {action.icon}
                        {action.label}
                    </button>
                ))}
                <button type="button" className={styles.clear} aria-label="Clear selection" onClick={onClear}>
                    <X size={14} />
                </button>
            </div>
        </Portal>
    );
}
