import { type ReactNode, useMemo } from "react";
import { Combobox, type ComboboxData } from "../ui";
import { type Coding, codingKey, type ExpandValueSet } from "./terminology";
import { useTerminologyExpansion } from "./useTerminologyExpansion";
import classes from "./CodingInput.module.css";

export interface CodingInputProps {
    value?: Coding | null;
    onChange?: (value: Coding | null) => void;
    /** Performs `ValueSet/$expand`. */
    expand: ExpandValueSet;
    /** Canonical URL of the ValueSet to expand. */
    valueSet?: string;
    /** Page size passed to `$expand`. Default 20. */
    count?: number;
    debounceMs?: number;
    minQueryLength?: number;

    placeholder?: string;
    disabled?: boolean;
    required?: boolean;
    clearable?: boolean;
    size?: "xs" | "sm" | "md" | "lg" | "xl";

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

export function CodingInput({
    value,
    onChange,
    expand,
    valueSet,
    count,
    debounceMs,
    minQueryLength,
    placeholder = "Search terminology…",
    clearable = true,
    ...rest
}: CodingInputProps) {
    const { setQuery, concepts, loading, query } = useTerminologyExpansion({
        expand,
        valueSet,
        count,
        debounceMs,
        minQueryLength,
    });

    const byKey = useMemo(() => {
        const map = new Map<string, Coding>();
        for (const c of concepts) map.set(codingKey(c), { system: c.system, code: c.code, display: c.display });
        if (value?.code != null && !map.has(codingKey(value))) map.set(codingKey(value), value);
        return map;
    }, [concepts, value]);

    const data = useMemo<ComboboxData>(() => {
        return Array.from(byKey.entries()).map(([key, c]) => {
            const display = c.display ?? c.code ?? key;
            return {
                value: key,
                textValue: display,
                label: (
                    <span className={classes.option}>
                        <span className={classes.optionTitle}>{display}</span>
                        {c.code && <span className={classes.optionCode}>{c.code}</span>}
                    </span>
                ),
            };
        });
    }, [byKey]);

    return (
        <Combobox
            {...rest}
            data={data}
            placeholder={placeholder}
            clearable={clearable}
            filter="server"
            loading={loading}
            emptyMessage={query.trim() ? "No matching concepts" : "Type to search"}
            value={value ? codingKey(value) : null}
            onInputChange={setQuery}
            onChange={(key) => {
                if (!key) {
                    onChange?.(null);
                    return;
                }
                onChange?.(byKey.get(key) ?? null);
            }}
        />
    );
}
