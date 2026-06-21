import { type ReactNode, useState } from "react";
import type { Size } from "../layout-utils";
import styles from "./SegmentedRadioGroup.module.css";

export interface SegmentedRadioGroupOption {
    value: string;
    label?: ReactNode;
    /** Alias of {@link label}. */
    content?: ReactNode;
    disabled?: boolean;
}

export interface SegmentedRadioGroupProps {
    options: SegmentedRadioGroupOption[];
    value?: string;
    defaultValue?: string;
    onChange?: (value: string) => void;
    /** Alias of {@link onChange}. */
    onUpdate?: (value: string) => void;
    size?: Size;
    disabled?: boolean;
    name?: string;
    className?: string;
    "aria-label"?: string;
}

export function SegmentedRadioGroup({
    options,
    value,
    defaultValue,
    onChange,
    onUpdate,
    size = "md",
    disabled,
    className,
    "aria-label": ariaLabel,
}: SegmentedRadioGroupProps) {
    const [internal, setInternal] = useState(defaultValue);
    const current = value ?? internal;
    const select = (next: string) => {
        if (value === undefined) setInternal(next);
        onChange?.(next);
        onUpdate?.(next);
    };
    return (
        <div
            role="radiogroup"
            aria-label={ariaLabel}
            data-size={size}
            className={[styles.group, className].filter(Boolean).join(" ")}
        >
            {options.map((option) => {
                const active = option.value === current;
                return (
                    <button
                        key={option.value}
                        type="button"
                        role="radio"
                        aria-checked={active}
                        disabled={disabled || option.disabled}
                        data-active={active || undefined}
                        className={styles.option}
                        onClick={() => select(option.value)}
                    >
                        {option.label ?? option.content ?? option.value}
                    </button>
                );
            })}
        </div>
    );
}
