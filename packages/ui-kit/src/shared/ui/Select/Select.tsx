import type { ReactNode } from "react";
import { Select as BaseSelect } from "@base-ui/react/select";
import { Check, ChevronDown } from "lucide-react";
import type { Size } from "../layout-utils";
import inputStyles from "../input.module.css";
import styles from "./Select.module.css";

export interface SelectOption {
    value: string;
    label?: ReactNode;
    content?: ReactNode;
    disabled?: boolean;
}

export interface SelectOptionGroup {
    group: string;
    items: SelectOption[];
}

export type SelectData = (SelectOption | SelectOptionGroup | string)[];

export interface SelectProps {
    value?: string | null;
    defaultValue?: string | null;
    onChange?: (value: string | null) => void;
    onUpdate?: (value: string | null) => void;
    /** Options; accepts `{value,label}`, plain strings, or `{group,items}`. */
    data?: SelectData;
    /** Alias of {@link data}. */
    options?: SelectData;
    placeholder?: string;
    disabled?: boolean;
    size?: Size;
    leftSection?: ReactNode;
    name?: string;
    id?: string;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

function isGroup(item: SelectOption | SelectOptionGroup): item is SelectOptionGroup {
    return "group" in item;
}

function normalize(data: SelectData | undefined): (SelectOption | SelectOptionGroup)[] {
    if (!data) return [];
    return data.map((item) => (typeof item === "string" ? { value: item, label: item } : item));
}

function renderOption(opt: SelectOption) {
    const label = opt.label ?? opt.content ?? opt.value;
    return (
        <BaseSelect.Item key={opt.value} value={opt.value} disabled={opt.disabled} className={styles.item}>
            <BaseSelect.ItemText>{label}</BaseSelect.ItemText>
            <BaseSelect.ItemIndicator className={styles.itemIndicator}>
                <Check size={14} />
            </BaseSelect.ItemIndicator>
        </BaseSelect.Item>
    );
}

export function Select({
    value,
    defaultValue,
    onChange,
    onUpdate,
    data,
    options,
    placeholder,
    disabled,
    size = "md",
    leftSection,
    name,
    id,
    className,
    style,
    ...props
}: SelectProps) {
    const items = normalize(data ?? options);
    const labels = new Map<string, ReactNode>();
    for (const item of items) {
        if (isGroup(item)) {
            for (const opt of item.items) labels.set(opt.value, opt.label ?? opt.content ?? opt.value);
        } else {
            labels.set(item.value, item.label ?? item.content ?? item.value);
        }
    }

    return (
        <BaseSelect.Root
            value={value ?? undefined}
            defaultValue={defaultValue ?? undefined}
            disabled={disabled}
            name={name}
            onValueChange={(next) => {
                const v = (next as string | null) ?? null;
                onChange?.(v);
                onUpdate?.(v);
            }}
        >
            <BaseSelect.Trigger
                id={id}
                className={[inputStyles.wrapper, styles.trigger, className].filter(Boolean).join(" ")}
                data-size={size}
                data-disabled={disabled ? "true" : undefined}
                style={style}
                {...props}
            >
                {leftSection != null && <span className={inputStyles.affix}>{leftSection}</span>}
                <span className={styles.value}>
                    <BaseSelect.Value>
                        {(val: unknown) => {
                            const key = typeof val === "string" ? val : null;
                            if (key && labels.has(key)) return labels.get(key);
                            return <span className={styles.placeholder}>{placeholder}</span>;
                        }}
                    </BaseSelect.Value>
                </span>
                <BaseSelect.Icon className={styles.chevron}>
                    <ChevronDown size={16} />
                </BaseSelect.Icon>
            </BaseSelect.Trigger>
            <BaseSelect.Portal>
                <BaseSelect.Positioner sideOffset={6}>
                    <BaseSelect.Popup className={styles.popup}>
                        {items.map((item) =>
                            isGroup(item) ? (
                                <BaseSelect.Group key={item.group}>
                                    <BaseSelect.GroupLabel className={styles.groupLabel}>
                                        {item.group}
                                    </BaseSelect.GroupLabel>
                                    {item.items.map(renderOption)}
                                </BaseSelect.Group>
                            ) : (
                                renderOption(item)
                            ),
                        )}
                    </BaseSelect.Popup>
                </BaseSelect.Positioner>
            </BaseSelect.Portal>
        </BaseSelect.Root>
    );
}
