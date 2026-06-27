import { type ReactNode, useMemo } from "react";
import { Autocomplete as BaseAutocomplete } from "@base-ui/react/autocomplete";
import { Search, X } from "lucide-react";
import { Field } from "../Field/Field";
import inputStyles from "../input.module.css";
import type { Size } from "../layout-utils";
import { Spin } from "../Spin";
import styles from "./Autocomplete.module.css";

export interface AutocompleteOption {
    value: string;
    label?: ReactNode;
    /** Plain-text used for filtering/input display; defaults to `value`. */
    textValue?: string;
    disabled?: boolean;
}

export type AutocompleteData = (AutocompleteOption | string)[];

export interface AutocompleteProps {
    /** Current free-text input value. */
    value?: string;
    defaultValue?: string;
    /** Fires on every input change (typing and selecting a suggestion). */
    onChange?: (value: string) => void;
    /** Fires when a suggestion is chosen; receives the option's canonical value. */
    onPick?: (value: string) => void;

    data?: AutocompleteData;
    /** Alias of {@link data}. */
    options?: AutocompleteData;

    placeholder?: string;
    disabled?: boolean;
    readOnly?: boolean;
    required?: boolean;
    size?: Size;
    clearable?: boolean;
    /** Left adornment; defaults to a search icon. */
    leftSection?: ReactNode;

    /** `"client"` (default) filters suggestions in-memory; `"server"` shows `data` as-is. */
    filter?: "client" | "server";
    loading?: boolean;
    emptyMessage?: ReactNode;

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

interface NormalizedOption {
    value: string;
    label: ReactNode;
    textValue: string;
    disabled?: boolean;
}

function normalize(data: AutocompleteData | undefined): NormalizedOption[] {
    if (!data) return [];
    return data.map((item) => {
        if (typeof item === "string") return { value: item, label: item, textValue: item };
        const textValue = item.textValue ?? (typeof item.label === "string" ? item.label : item.value);
        return { value: item.value, label: item.label ?? item.value, textValue, disabled: item.disabled };
    });
}

export function Autocomplete({
    value,
    defaultValue,
    onChange,
    onPick,
    data,
    options,
    placeholder,
    disabled,
    readOnly,
    required,
    size = "md",
    clearable,
    leftSection,
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
    "aria-label": ariaLabel,
}: AutocompleteProps) {
    const items = useMemo(() => normalize(data ?? options), [data, options]);
    const itemToStringValue = useMemo(() => {
        const map = new Map(items.map((o) => [o.value, o.textValue]));
        return (val: unknown) => {
            if (typeof val !== "string") return "";
            return map.get(val) ?? val;
        };
    }, [items]);

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
            <BaseAutocomplete.Root
                items={items.map((o) => o.value)}
                value={value}
                defaultValue={defaultValue}
                disabled={disabled}
                readOnly={readOnly}
                name={name}
                mode={filter === "server" ? "none" : "list"}
                itemToStringValue={itemToStringValue}
                onValueChange={(next) => onChange?.(next)}
            >
                <div
                    className={[inputStyles.wrapper, styles.control].filter(Boolean).join(" ")}
                    data-size={size}
                    data-error={error ? "true" : undefined}
                    data-disabled={disabled ? "true" : undefined}
                >
                    <span className={inputStyles.affix}>{leftSection ?? <Search size={16} />}</span>
                    <BaseAutocomplete.Input
                        id={id}
                        placeholder={placeholder}
                        className={[inputStyles.input, styles.input].join(" ")}
                        aria-label={ariaLabel}
                    />
                    {clearable && (
                        <BaseAutocomplete.Clear className={styles.clear} aria-label="Clear">
                            <X size={14} />
                        </BaseAutocomplete.Clear>
                    )}
                </div>

                <BaseAutocomplete.Portal>
                    <BaseAutocomplete.Positioner sideOffset={6} className={styles.positioner}>
                        <BaseAutocomplete.Popup className={styles.popup}>
                            {loading && (
                                <div className={styles.status}>
                                    <Spin size="xs" /> Searching…
                                </div>
                            )}
                            <BaseAutocomplete.Empty className={styles.empty}>{emptyMessage}</BaseAutocomplete.Empty>
                            <BaseAutocomplete.List className={styles.list}>
                                {(itemValue: string) => {
                                    const opt = items.find((o) => o.value === itemValue);
                                    if (!opt) return null;
                                    return (
                                        <BaseAutocomplete.Item
                                            key={opt.value}
                                            value={opt.value}
                                            disabled={opt.disabled}
                                            className={styles.item}
                                            onClick={() => onPick?.(opt.value)}
                                        >
                                            {opt.label}
                                        </BaseAutocomplete.Item>
                                    );
                                }}
                            </BaseAutocomplete.List>
                        </BaseAutocomplete.Popup>
                    </BaseAutocomplete.Positioner>
                </BaseAutocomplete.Portal>
            </BaseAutocomplete.Root>
        </Field>
    );
}
