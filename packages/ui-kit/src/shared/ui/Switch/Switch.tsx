import { forwardRef, useId, type ReactNode } from "react";
import { Switch as BaseSwitch } from "@base-ui/react/switch";
import styles from "./Switch.module.css";

export interface SwitchProps {
    checked?: boolean;
    defaultChecked?: boolean;
    onChange?: (checked: boolean) => void;
    onUpdate?: (checked: boolean) => void;
    disabled?: boolean;
    name?: string;
    size?: "s" | "m";
    label?: ReactNode;
    content?: ReactNode;
    description?: ReactNode;
    className?: string;
}

export const Switch = forwardRef<HTMLButtonElement, SwitchProps>(function Switch(
    { checked, defaultChecked, onChange, onUpdate, disabled, name, size = "m", label, content, description, className },
    ref,
) {
    const text = label ?? content;
    const id = useId();
    const hasText = text != null || description != null;

    const control = (
        <BaseSwitch.Root
            ref={ref}
            id={hasText ? id : undefined}
            className={styles.control}
            data-size={size}
            checked={checked}
            defaultChecked={defaultChecked}
            disabled={disabled}
            name={name}
            onCheckedChange={(next) => {
                onChange?.(next);
                onUpdate?.(next);
            }}
        >
            <BaseSwitch.Thumb className={styles.thumb} />
        </BaseSwitch.Root>
    );

    if (!hasText) return control;

    return (
        <span className={[styles.root, className].filter(Boolean).join(" ")} data-disabled={disabled ? "" : undefined}>
            {control}
            <span className={styles.text}>
                {text != null && (
                    <label className={styles.label} htmlFor={id}>
                        {text}
                    </label>
                )}
                {description != null && <span className={styles.description}>{description}</span>}
            </span>
        </span>
    );
});
