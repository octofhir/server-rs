import { forwardRef, type ReactNode } from "react";
import { Field } from "../Field/Field";
import type { Size } from "../layout-utils";
import styles from "../input.module.css";

export interface TextAreaProps
    extends Omit<React.TextareaHTMLAttributes<HTMLTextAreaElement>, "onChange" | "size"> {
    value?: string;
    defaultValue?: string;
    onChange?: (value: string) => void;
    onUpdate?: (value: string) => void;
    size?: Size;
    error?: boolean | string;
    minRows?: number;
    maxRows?: number;
    label?: ReactNode;
    description?: ReactNode;
    required?: boolean;
    w?: number | string;
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
    {
        value,
        defaultValue,
        onChange,
        onUpdate,
        size = "md",
        error,
        disabled,
        rows,
        minRows,
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
            <div
                className={[styles.wrapper, styles.block].filter(Boolean).join(" ")}
                data-size={size}
                data-error={error ? "true" : undefined}
                data-disabled={disabled ? "true" : undefined}
            >
                <textarea
                    ref={ref}
                    className={[styles.input, styles.textarea].join(" ")}
                    value={value}
                    defaultValue={defaultValue}
                    disabled={disabled}
                    rows={rows ?? minRows ?? 3}
                    onChange={(e) => {
                        onChange?.(e.currentTarget.value);
                        onUpdate?.(e.currentTarget.value);
                    }}
                    {...props}
                />
            </div>
        </Field>
    );
});
