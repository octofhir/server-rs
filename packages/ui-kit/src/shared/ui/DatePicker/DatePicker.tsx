import { forwardRef, type ReactNode } from "react";
import { X } from "lucide-react";
import type { Size } from "../layout-utils";
import inputStyles from "../input.module.css";
import styles from "./DatePicker.module.css";

export interface DatePickerProps {
    value?: Date | null;
    defaultValue?: Date | null;
    onChange?: (date: Date | null) => void;
    /** Alias of {@link onChange}. */
    onUpdate?: (date: Date | null) => void;
    /** Include a time component (renders a datetime field). */
    withTime?: boolean;
    label?: ReactNode;
    placeholder?: string;
    size?: Size;
    disabled?: boolean;
    /** Show a button to reset the value to empty. */
    clearable?: boolean;
    minDate?: Date;
    maxDate?: Date;
    error?: boolean | string;
    name?: string;
    id?: string;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

function pad(n: number): string {
    return String(n).padStart(2, "0");
}

function toInputValue(date: Date | null | undefined, withTime: boolean): string {
    if (!date || Number.isNaN(date.getTime())) return "";
    const ymd = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
    if (!withTime) return ymd;
    return `${ymd}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export const DatePicker = forwardRef<HTMLInputElement, DatePickerProps>(function DatePicker(
    {
        value,
        defaultValue,
        onChange,
        onUpdate,
        withTime = false,
        label,
        placeholder,
        size = "md",
        disabled,
        clearable,
        minDate,
        maxDate,
        error,
        name,
        id,
        className,
        style,
        ...props
    },
    ref,
) {
    const emit = (next: Date | null) => {
        onChange?.(next);
        onUpdate?.(next);
    };
    const handleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
        const raw = event.currentTarget.value;
        if (!raw) return emit(null);
        const parsed = new Date(raw);
        emit(Number.isNaN(parsed.getTime()) ? null : parsed);
    };

    return (
        <div className={[styles.field, className].filter(Boolean).join(" ")} style={style}>
            {label != null && (
                <label className={styles.label} htmlFor={id}>
                    {label}
                </label>
            )}
            <div
                className={inputStyles.wrapper}
                data-size={size}
                data-error={error ? "true" : undefined}
                data-disabled={disabled ? "true" : undefined}
            >
                <input
                    ref={ref}
                    id={id}
                    name={name}
                    type={withTime ? "datetime-local" : "date"}
                    className={[inputStyles.input, styles.input].join(" ")}
                    value={value !== undefined ? toInputValue(value, withTime) : undefined}
                    defaultValue={
                        defaultValue !== undefined ? toInputValue(defaultValue, withTime) : undefined
                    }
                    placeholder={placeholder}
                    disabled={disabled}
                    min={minDate ? toInputValue(minDate, withTime) : undefined}
                    max={maxDate ? toInputValue(maxDate, withTime) : undefined}
                    onChange={handleChange}
                    {...props}
                />
                {clearable && value != null && !disabled && (
                    <button
                        type="button"
                        className={inputStyles.iconButton}
                        aria-label="Clear date"
                        onClick={() => emit(null)}
                    >
                        <X size={14} />
                    </button>
                )}
            </div>
        </div>
    );
});

export const DateTimePicker = forwardRef<HTMLInputElement, DatePickerProps>(function DateTimePicker(
    props,
    ref,
) {
    return <DatePicker ref={ref} withTime {...props} />;
});
