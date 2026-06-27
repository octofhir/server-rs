import { type ReactNode, useMemo } from "react";
import { Combobox as BaseCombobox } from "@base-ui/react/combobox";
import { Check, ChevronDown, X } from "lucide-react";
import { Field } from "../Field/Field";
import inputStyles from "../input.module.css";
import type { Size } from "../layout-utils";
import { Spin } from "../Spin";
import styles from "./Combobox.module.css";

export interface ComboboxOption {
    value: string;
    label?: ReactNode;
    /** Plain-text label used for filtering/input display; defaults to `label` when it is a string. */
    textValue?: string;
    disabled?: boolean;
}

export type ComboboxData = (ComboboxOption | string)[];

interface BaseComboboxProps {
    data?: ComboboxData;
    /** Alias of {@link data}. */
    options?: ComboboxData;
    placeholder?: string;
    disabled?: boolean;
    readOnly?: boolean;
    required?: boolean;
    size?: Size;
    /** Allow clearing the selection. */
    clearable?: boolean;
    /** Left adornment inside the control. */
    leftSection?: ReactNode;

    // async / server-side filtering
    /** Called with the typed query. Pair with `filter="server"` for remote search. */
    onInputChange?: (query: string) => void;
    /** `"client"` (default) filters in-memory; `"server"` disables local filtering. */
    filter?: "client" | "server";
    /** Show a loading indicator in the popup. */
    loading?: boolean;
    /** Message shown when no options match. */
    emptyMessage?: ReactNode;

    // field
    label?: ReactNode;
    description?: ReactNode;
    error?: boolean | string;
    w?: number | string;

    name?: string;
    id?: string;
    className?: string;
    style?: React.CSSProperties;
    "aria-label"?: string;
}

export interface ComboboxSingleProps extends BaseComboboxProps {
    multiple?: false;
    value?: string | null;
    defaultValue?: string | null;
    onChange?: (value: string | null) => void;
}

export interface ComboboxMultipleProps extends BaseComboboxProps {
    multiple: true;
    value?: string[] | null;
    defaultValue?: string[] | null;
    onChange?: (value: string[]) => void;
}

export type ComboboxProps = ComboboxSingleProps | ComboboxMultipleProps;

interface NormalizedOption {
    value: string;
    label: ReactNode;
    textValue: string;
    disabled?: boolean;
}

function normalize(data: ComboboxData | undefined): NormalizedOption[] {
    if (!data) return [];
    return data.map((item) => {
        if (typeof item === "string") return { value: item, label: item, textValue: item };
        const textValue =
            item.textValue ?? (typeof item.label === "string" ? item.label : item.value);
        return { value: item.value, label: item.label ?? item.value, textValue, disabled: item.disabled };
    });
}

export function Combobox(props: ComboboxProps) {
    const {
        data,
        options,
        placeholder,
        disabled,
        readOnly,
        required,
        size = "md",
        clearable,
        leftSection,
        onInputChange,
        filter = "client",
        loading,
        emptyMessage = "No results",
        label,
        description,
        error,
        w,
        name,
        id,
        className,
        style,
        multiple,
        value,
        defaultValue,
        "aria-label": ariaLabel,
    } = props;

    const items = useMemo(() => normalize(data ?? options), [data, options]);
    const itemToStringLabel = useMemo(() => {
        const map = new Map(items.map((o) => [o.value, o.textValue]));
        return (val: string) => map.get(val) ?? val;
    }, [items]);

    const handleValueChange = (next: unknown) => {
        if (multiple) {
            (props.onChange as ((v: string[]) => void) | undefined)?.((next as string[]) ?? []);
        } else {
            (props.onChange as ((v: string | null) => void) | undefined)?.((next as string | null) ?? null);
        }
    };

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
            <BaseCombobox.Root
                items={items}
                multiple={multiple as never}
                value={value as never}
                defaultValue={defaultValue as never}
                disabled={disabled}
                readOnly={readOnly}
                name={name}
                itemToStringLabel={itemToStringLabel}
                filter={filter === "server" ? null : undefined}
                onValueChange={handleValueChange}
                onInputValueChange={onInputChange ? (text) => onInputChange(text) : undefined}
            >
                <div
                    className={[inputStyles.wrapper, styles.control].filter(Boolean).join(" ")}
                    data-size={size}
                    data-error={error ? "true" : undefined}
                    data-disabled={disabled ? "true" : undefined}
                >
                    {leftSection != null && <span className={inputStyles.affix}>{leftSection}</span>}
                    {multiple ? (
                        <BaseCombobox.Chips className={styles.chips}>
                            <ComboboxChipList itemToStringLabel={itemToStringLabel} />
                            <BaseCombobox.Input
                                id={id}
                                placeholder={placeholder}
                                className={[inputStyles.input, styles.input].join(" ")}
                                aria-label={ariaLabel}
                            />
                        </BaseCombobox.Chips>
                    ) : (
                        <BaseCombobox.Input
                            id={id}
                            placeholder={placeholder}
                            className={[inputStyles.input, styles.input].join(" ")}
                            aria-label={ariaLabel}
                        />
                    )}
                    {clearable && (
                        <BaseCombobox.Clear className={styles.clear} aria-label="Clear">
                            <X size={14} />
                        </BaseCombobox.Clear>
                    )}
                    <BaseCombobox.Trigger className={styles.trigger} aria-label="Open">
                        <ChevronDown size={16} className={styles.chevron} />
                    </BaseCombobox.Trigger>
                </div>

                <BaseCombobox.Portal>
                    <BaseCombobox.Positioner sideOffset={6} className={styles.positioner}>
                        <BaseCombobox.Popup className={styles.popup}>
                            {loading && (
                                <div className={styles.status}>
                                    <Spin size="xs" /> Searching…
                                </div>
                            )}
                            <BaseCombobox.Empty className={styles.empty}>{emptyMessage}</BaseCombobox.Empty>
                            <BaseCombobox.List className={styles.list}>
                                {(item: NormalizedOption) => (
                                    <BaseCombobox.Item
                                        key={item.value}
                                        value={item.value}
                                        disabled={item.disabled}
                                        className={styles.item}
                                    >
                                        <BaseCombobox.ItemIndicator className={styles.itemIndicator}>
                                            <Check size={14} />
                                        </BaseCombobox.ItemIndicator>
                                        <span className={styles.itemLabel}>{item.label}</span>
                                    </BaseCombobox.Item>
                                )}
                            </BaseCombobox.List>
                        </BaseCombobox.Popup>
                    </BaseCombobox.Positioner>
                </BaseCombobox.Portal>
            </BaseCombobox.Root>
        </Field>
    );
}

function ComboboxChipList({ itemToStringLabel }: { itemToStringLabel: (val: string) => string }) {
    return (
        <BaseCombobox.Value>
            {(value: unknown) => {
                const values = Array.isArray(value) ? (value as string[]) : [];
                return (
                    <>
                        {values.map((val) => (
                            <BaseCombobox.Chip key={val} className={styles.chip}>
                                {itemToStringLabel(val)}
                                <BaseCombobox.ChipRemove className={styles.chipRemove} aria-label="Remove">
                                    <X size={12} />
                                </BaseCombobox.ChipRemove>
                            </BaseCombobox.Chip>
                        ))}
                    </>
                );
            }}
        </BaseCombobox.Value>
    );
}
