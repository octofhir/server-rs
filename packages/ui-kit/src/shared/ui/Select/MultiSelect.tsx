import type { ReactNode } from "react";
import { Combobox } from "@base-ui/react/combobox";
import { Check, ChevronDown, X } from "lucide-react";
import inputStyles from "../input.module.css";
import styles from "./MultiSelect.module.css";
import selectStyles from "./Select.module.css";
import type { SelectData, SelectOption, SelectOptionGroup } from "./Select";

export interface MultiSelectProps {
    value?: string[];
    defaultValue?: string[];
    onChange?: (value: string[]) => void;
    /** Alias of {@link onChange}. */
    onUpdate?: (value: string[]) => void;
    data?: SelectData;
    /** Alias of {@link data}. */
    options?: SelectData;
    placeholder?: string;
    label?: ReactNode;
    required?: boolean;
    error?: boolean | string;
    disabled?: boolean;
    size?: "s" | "m" | "l";
    className?: string;
    style?: React.CSSProperties;
    id?: string;
    name?: string;
    "aria-label"?: string;
}

function isGroup(item: SelectOption | SelectOptionGroup): item is SelectOptionGroup {
    return "group" in item;
}

function flatten(data: SelectData | undefined): SelectOption[] {
    if (!data) return [];
    const out: SelectOption[] = [];
    for (const entry of data) {
        const item = typeof entry === "string" ? { value: entry, label: entry } : entry;
        if (isGroup(item)) out.push(...item.items);
        else out.push(item);
    }
    return out;
}

export function MultiSelect({
    value,
    defaultValue,
    onChange,
    onUpdate,
    data,
    options,
    placeholder,
    label,
    required,
    error,
    disabled,
    size = "m",
    className,
    style,
    id,
    name,
    "aria-label": ariaLabel,
}: MultiSelectProps) {
    const items = flatten(data ?? options);
    const labels = new Map<string, ReactNode>();
    for (const item of items) labels.set(item.value, item.label ?? item.content ?? item.value);
    const itemValues = items.map((item) => item.value);

    const selected = value ?? [];
    const emit = (next: string[]) => {
        onChange?.(next);
        onUpdate?.(next);
    };

    return (
        <div className={[styles.field, className].filter(Boolean).join(" ")} style={style}>
            {label != null && (
                <span className={styles.label}>
                    {label}
                    {required && <span className={styles.required}>*</span>}
                </span>
            )}
            <Combobox.Root
                items={itemValues}
                multiple
                value={value}
                defaultValue={defaultValue}
                disabled={disabled}
                name={name}
                onValueChange={(next) => emit((next as string[]) ?? [])}
            >
                <div
                    className={[inputStyles.wrapper, styles.control].join(" ")}
                    data-size={size}
                    data-error={error ? "true" : undefined}
                    data-disabled={disabled ? "true" : undefined}
                >
                    <Combobox.Chips className={styles.chips}>
                        {selected.map((val) => (
                            <Combobox.Chip key={val} className={styles.chip}>
                                {labels.get(val) ?? val}
                                <Combobox.ChipRemove className={styles.chipRemove} aria-label="Remove">
                                    <X size={12} />
                                </Combobox.ChipRemove>
                            </Combobox.Chip>
                        ))}
                        <Combobox.Input
                            id={id}
                            aria-label={ariaLabel}
                            placeholder={selected.length === 0 ? placeholder : undefined}
                            disabled={disabled}
                            className={[inputStyles.input, styles.input].join(" ")}
                        />
                    </Combobox.Chips>
                    <Combobox.Icon className={styles.chevron}>
                        <ChevronDown size={16} />
                    </Combobox.Icon>
                </div>
                <Combobox.Portal>
                    <Combobox.Positioner sideOffset={6}>
                        <Combobox.Popup className={selectStyles.popup}>
                            <Combobox.Empty className={styles.empty}>No options</Combobox.Empty>
                            <Combobox.List>
                                {(item: string) => (
                                    <Combobox.Item key={item} value={item} className={selectStyles.item}>
                                        <span>{labels.get(item) ?? item}</span>
                                        <Combobox.ItemIndicator className={selectStyles.itemIndicator}>
                                            <Check size={14} />
                                        </Combobox.ItemIndicator>
                                    </Combobox.Item>
                                )}
                            </Combobox.List>
                        </Combobox.Popup>
                    </Combobox.Positioner>
                </Combobox.Portal>
            </Combobox.Root>
        </div>
    );
}
