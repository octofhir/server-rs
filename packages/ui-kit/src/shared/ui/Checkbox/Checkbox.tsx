import { forwardRef, useId, type ReactNode } from "react";
import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { Check, Minus } from "lucide-react";
import styles from "./Checkbox.module.css";

export interface CheckboxProps {
    checked?: boolean;
    defaultChecked?: boolean;
    indeterminate?: boolean;
    onChange?: (checked: boolean) => void;
    onUpdate?: (checked: boolean) => void;
    disabled?: boolean;
    name?: string;
    label?: ReactNode;
    content?: ReactNode;
    className?: string;
}

export const Checkbox = forwardRef<HTMLButtonElement, CheckboxProps>(function Checkbox(
    { checked, defaultChecked, indeterminate, onChange, onUpdate, disabled, name, label, content, className },
    ref,
) {
    const text = label ?? content;
    const id = useId();

    const control = (
        <BaseCheckbox.Root
            ref={ref}
            id={text != null ? id : undefined}
            className={styles.control}
            checked={checked}
            defaultChecked={defaultChecked}
            indeterminate={indeterminate}
            disabled={disabled}
            name={name}
            onCheckedChange={(next) => {
                onChange?.(next);
                onUpdate?.(next);
            }}
        >
            <BaseCheckbox.Indicator className={styles.indicator}>
                {indeterminate ? <Minus size={14} /> : <Check size={14} />}
            </BaseCheckbox.Indicator>
        </BaseCheckbox.Root>
    );

    if (text == null) return control;

    return (
        <span className={[styles.root, className].filter(Boolean).join(" ")} data-disabled={disabled ? "" : undefined}>
            {control}
            <label htmlFor={id}>{text}</label>
        </span>
    );
});
