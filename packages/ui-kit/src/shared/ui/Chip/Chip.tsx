import { forwardRef, type ReactNode } from "react";
import styles from "./Chip.module.css";

export interface ChipProps {
    /** Selected state. */
    checked?: boolean;
    /** Toggle callback. Receives the new checked value. */
    onChange?: (next: boolean) => void;
    size?: "xs" | "s" | "m";
    disabled?: boolean;
    /** Content rendered before the label. */
    leftSection?: ReactNode;
    children?: ReactNode;
    className?: string;
    "aria-label"?: string;
}

export const Chip = forwardRef<HTMLButtonElement, ChipProps>(function Chip(
    { checked = false, onChange, size = "s", disabled, leftSection, children, className, ...rest },
    ref,
) {
    return (
        <button
            ref={ref}
            type="button"
            disabled={disabled}
            data-size={size}
            data-checked={checked || undefined}
            aria-pressed={checked}
            className={[styles.chip, className].filter(Boolean).join(" ")}
            onClick={() => onChange?.(!checked)}
            {...rest}
        >
            {leftSection}
            {children}
        </button>
    );
});
