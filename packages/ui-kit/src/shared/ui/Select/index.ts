import React, { type ReactNode } from "react";
import {
    Select as GravitySelect,
    type SelectOption,
    type SelectOptionGroup,
    type SelectProps as GravitySelectProps,
} from "@gravity-ui/uikit";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";

type LegacyDataItem =
    | string
    | {
          value: string;
          label?: ReactNode;
          disabled?: boolean;
          title?: string;
      }
    | {
          group: string;
          items: LegacyDataItem[];
      };

export interface SelectProps
    extends Omit<GravitySelectProps, "value" | "defaultValue" | "options" | "onUpdate" | "onChange"> {
    value?: string | string[] | null;
    defaultValue?: string | string[] | null;
    data?: LegacyDataItem[];
    options?: GravitySelectProps["options"];
    onChange?: (value: string | string[] | null) => void;
    onUpdate?: (value: string[]) => void;
    searchable?: boolean;
    clearable?: boolean;
    allowDeselect?: boolean;
    withCheckIcon?: boolean;
    leftSection?: ReactNode;
    rightSection?: ReactNode;
    variant?: string;
    styles?: Record<string, React.CSSProperties>;
    style?: React.CSSProperties;
    w?: number | string;
    h?: number | string;
    p?: number | string;
    px?: number | string;
    py?: number | string;
    pt?: number | string;
    pb?: number | string;
    pl?: number | string;
    pr?: number | string;
    m?: number | string;
    mx?: number | string;
    my?: number | string;
    mt?: number | string;
    mb?: number | string;
    ml?: number | string;
    mr?: number | string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
}

function isLegacyDataItem(item: unknown): item is LegacyDataItem {
    if (typeof item === "string") return true;
    if (!isRecord(item)) return false;
    if ("group" in item) {
        return typeof item.group === "string" && Array.isArray(item.items) && item.items.every(isLegacyDataItem);
    }
    return typeof item.value === "string";
}

function toGravityOption(item: LegacyDataItem): SelectOption | SelectOptionGroup {
    if (typeof item === "string") {
        return { value: item, content: item };
    }
    if ("group" in item) {
        return {
            label: item.group,
            options: item.items.map((child) => toGravityOption(child) as SelectOption),
        };
    }
    return {
        value: item.value,
        content: item.label ?? item.value,
        disabled: item.disabled,
        title: item.title,
    };
}

function toGravityValue(value: SelectProps["value"]): string[] {
    if (Array.isArray(value)) return value;
    return value ? [value] : [];
}

function toGravityOptions(props: Pick<SelectProps, "data" | "options">): GravitySelectProps["options"] {
    if (Array.isArray(props.options)) return props.options;
    if (!Array.isArray(props.data)) return undefined;
    return props.data.filter(isLegacyDataItem).map(toGravityOption);
}

export function Select({
    data,
    options,
    value,
    defaultValue,
    onChange,
    onUpdate,
    searchable,
    clearable,
    allowDeselect: _allowDeselect,
    withCheckIcon: _withCheckIcon,
    leftSection: _leftSection,
    rightSection: _rightSection,
    variant: _variant,
    styles,
    style,
    multiple,
    ...props
}: SelectProps) {
    const cleaned = cleanLayoutProps(props);
    const mergedStyle = {
        ...getSpacingStyles(props),
        ...styles?.root,
        ...style,
    };

    const select = React.createElement(GravitySelect, {
        ...cleaned,
        multiple,
        value: toGravityValue(value),
        defaultValue: defaultValue === undefined ? undefined : toGravityValue(defaultValue),
        options: toGravityOptions({ data, options }),
        filterable: searchable || props.filterable,
        hasClear: clearable || props.hasClear,
        onUpdate: (nextValue: string[]) => {
            onUpdate?.(nextValue);
            onChange?.(multiple ? nextValue : nextValue[0] ?? null);
        },
    } satisfies GravitySelectProps);

    return React.createElement(
        "span",
        {
            style: { display: "inline-block", ...mergedStyle },
        },
        select,
    );
}

export function MultiSelect(props: SelectProps) {
    return React.createElement(Select, { ...props, multiple: true });
}
