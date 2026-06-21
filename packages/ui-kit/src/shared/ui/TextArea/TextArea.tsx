import { forwardRef } from "react";
import styles from "../input.module.css";

export interface TextAreaProps
    extends Omit<React.TextareaHTMLAttributes<HTMLTextAreaElement>, "onChange" | "size"> {
    value?: string;
    defaultValue?: string;
    onChange?: (value: string) => void;
    onUpdate?: (value: string) => void;
    size?: "s" | "m" | "l";
    error?: boolean | string;
    minRows?: number;
    maxRows?: number;
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
    { value, defaultValue, onChange, onUpdate, size = "m", error, disabled, rows, minRows, className, style, ...props },
    ref,
) {
    return (
        <div
            className={[styles.wrapper, styles.block, className].filter(Boolean).join(" ")}
            data-size={size}
            data-error={error ? "true" : undefined}
            data-disabled={disabled ? "true" : undefined}
            style={style}
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
    );
});
