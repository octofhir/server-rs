import { type ReactNode, useMemo } from "react";
import { Combobox, type ComboboxData, TextInput } from "../ui";
import { type CodeableConcept, type Coding, codingKey, type ExpandValueSet } from "./terminology";
import { useTerminologyExpansion } from "./useTerminologyExpansion";
import classes from "./CodingInput.module.css";

export interface CodeableConceptInputProps {
    value?: CodeableConcept | null;
    onChange?: (value: CodeableConcept | null) => void;
    /** Performs `ValueSet/$expand`. */
    expand: ExpandValueSet;
    valueSet?: string;
    count?: number;
    debounceMs?: number;
    minQueryLength?: number;

    /** Show the free-text input that maps to `CodeableConcept.text`. Default true. */
    withText?: boolean;
    textLabel?: ReactNode;

    placeholder?: string;
    disabled?: boolean;
    required?: boolean;
    size?: "xs" | "sm" | "md" | "lg" | "xl";

    label?: ReactNode;
    description?: ReactNode;
    error?: boolean | string;
    w?: number | string;
    name?: string;
    id?: string;
    className?: string;
    style?: React.CSSProperties;
}

function emit(coding: Coding[], text: string | undefined): CodeableConcept | null {
    const next: CodeableConcept = {};
    if (coding.length) next.coding = coding;
    if (text) next.text = text;
    return next.coding || next.text ? next : null;
}

export function CodeableConceptInput({
    value,
    onChange,
    expand,
    valueSet,
    count,
    debounceMs,
    minQueryLength,
    withText = true,
    textLabel = "Text",
    placeholder = "Search terminology…",
    size,
    ...rest
}: CodeableConceptInputProps) {
    const { setQuery, concepts, loading, query } = useTerminologyExpansion({
        expand,
        valueSet,
        count,
        debounceMs,
        minQueryLength,
    });

    const selected = useMemo(() => value?.coding ?? [], [value]);

    const byKey = useMemo(() => {
        const map = new Map<string, Coding>();
        for (const c of concepts) map.set(codingKey(c), { system: c.system, code: c.code, display: c.display });
        for (const c of selected) if (c.code != null && !map.has(codingKey(c))) map.set(codingKey(c), c);
        return map;
    }, [concepts, selected]);

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

    const selectedKeys = useMemo(() => selected.map((c) => codingKey(c)), [selected]);

    return (
        <div className={classes.stack}>
            <Combobox
                {...rest}
                multiple
                size={size}
                data={data}
                placeholder={placeholder}
                filter="server"
                loading={loading}
                emptyMessage={query.trim() ? "No matching concepts" : "Type to search"}
                value={selectedKeys}
                onInputChange={setQuery}
                onChange={(keys) => {
                    const coding = keys.map((k) => byKey.get(k)).filter((c): c is Coding => c != null);
                    onChange?.(emit(coding, value?.text));
                }}
            />
            {withText && (
                <TextInput
                    size={size}
                    label={textLabel}
                    value={value?.text ?? ""}
                    onChange={(text) => onChange?.(emit(selected, text || undefined))}
                />
            )}
        </div>
    );
}
