import { forwardRef, useState, type ReactNode } from "react";
import { Input as BaseInput } from "@base-ui/react/input";
import { Eye, EyeOff } from "lucide-react";
import styles from "../input.module.css";

export interface PasswordInputProps {
    value?: string;
    defaultValue?: string;
    onChange?: (value: string) => void;
    onUpdate?: (value: string) => void;
    placeholder?: string;
    disabled?: boolean;
    size?: "s" | "m" | "l";
    name?: string;
    id?: string;
    autoComplete?: string;
    error?: boolean | string;
    leftSection?: ReactNode;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

export const PasswordInput = forwardRef<HTMLInputElement, PasswordInputProps>(function PasswordInput(
    { value, defaultValue, onChange, onUpdate, size = "m", error, disabled, leftSection, className, style, ...props },
    ref,
) {
    const [visible, setVisible] = useState(false);
    return (
        <div
            className={[styles.wrapper, className].filter(Boolean).join(" ")}
            data-size={size}
            data-error={error ? "true" : undefined}
            data-disabled={disabled ? "true" : undefined}
            style={style}
        >
            {leftSection != null && <span className={styles.affix}>{leftSection}</span>}
            <BaseInput
                ref={ref}
                className={styles.input}
                type={visible ? "text" : "password"}
                value={value}
                defaultValue={defaultValue}
                disabled={disabled}
                onValueChange={(next) => {
                    onChange?.(next);
                    onUpdate?.(next);
                }}
                {...props}
            />
            <button
                type="button"
                className={styles.iconButton}
                aria-label={visible ? "Hide password" : "Show password"}
                onClick={() => setVisible((v) => !v)}
                tabIndex={-1}
            >
                {visible ? <EyeOff size={16} /> : <Eye size={16} />}
            </button>
        </div>
    );
});
