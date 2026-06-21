import { forwardRef, type ReactNode } from "react";
import { NumberField } from "@base-ui/react/number-field";
import { Minus, Plus } from "lucide-react";
import { Field } from "../Field/Field";
import type { Size } from "../layout-utils";
import styles from "../input.module.css";

export interface NumberInputProps {
    value?: number | null;
    defaultValue?: number;
    onChange?: (value: number | null) => void;
    onUpdate?: (value: number | null) => void;
    min?: number;
    max?: number;
    step?: number;
    disabled?: boolean;
    placeholder?: string;
    size?: Size;
    error?: boolean | string;
    label?: ReactNode;
    description?: ReactNode;
    required?: boolean;
    w?: number | string;
    name?: string;
    id?: string;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

export const NumberInput = forwardRef<HTMLInputElement, NumberInputProps>(function NumberInput(
    {
        value,
        defaultValue,
        onChange,
        onUpdate,
        min,
        max,
        step,
        disabled,
        size = "md",
        error,
        label,
        description,
        required,
        w,
        className,
        style,
        ...props
    },
    ref,
) {
    return (
        <Field
            label={label}
            description={description}
            error={error}
            required={required}
            className={className}
            style={style}
            w={w}
        >
            <NumberField.Root
                value={value}
                defaultValue={defaultValue}
                min={min}
                max={max}
                step={step}
                disabled={disabled}
                onValueChange={(next) => {
                    onChange?.(next);
                    onUpdate?.(next);
                }}
            >
                <NumberField.Group
                    className={styles.wrapper}
                    data-size={size}
                    data-error={error ? "true" : undefined}
                    data-disabled={disabled ? "true" : undefined}
                >
                    <NumberField.Decrement className={styles.iconButton} aria-label="Decrement">
                        <Minus size={14} />
                    </NumberField.Decrement>
                    <NumberField.Input ref={ref} className={styles.input} {...props} />
                    <NumberField.Increment className={styles.iconButton} aria-label="Increment">
                        <Plus size={14} />
                    </NumberField.Increment>
                </NumberField.Group>
            </NumberField.Root>
        </Field>
    );
});
