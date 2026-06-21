import type { CSSProperties, ReactNode } from "react";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import { cleanLayoutProps, getSpacingStyles, type Size, type SpacingProps } from "../layout-utils";
import styles from "./Tabs.module.css";

export interface TabsProps extends SpacingProps {
    value?: string | null;
    defaultValue?: string | null;
    onChange?: (value: string | null) => void;
    onUpdate?: (value: string) => void;
    size?: Size;
    children?: ReactNode;
    className?: string;
    style?: CSSProperties;
}

function TabsRoot({ value, defaultValue, onChange, onUpdate, size, children, className, style, ...rest }: TabsProps) {
    return (
        <BaseTabs.Root
            value={value ?? undefined}
            defaultValue={defaultValue ?? undefined}
            onValueChange={(next) => {
                const v = String(next);
                onChange?.(v);
                onUpdate?.(v);
            }}
            data-size={size}
            className={className}
            style={{ ...getSpacingStyles(rest), ...style }}
            {...cleanLayoutProps(rest)}
        >
            {children}
        </BaseTabs.Root>
    );
}

export interface TabsListProps {
    children?: ReactNode;
    className?: string;
}

function TabsList({ children, className }: TabsListProps) {
    return <BaseTabs.List className={[styles.list, className].filter(Boolean).join(" ")}>{children}</BaseTabs.List>;
}

export interface TabsTabProps {
    value: string;
    id?: string;
    children?: ReactNode;
    leftSection?: ReactNode;
    icon?: ReactNode;
    disabled?: boolean;
    className?: string;
}

function TabsTab({ value, id, children, leftSection, icon, disabled, className }: TabsTabProps) {
    const lead = icon ?? leftSection;
    return (
        <BaseTabs.Tab id={id} value={value} disabled={disabled} className={[styles.tab, className].filter(Boolean).join(" ")}>
            {lead}
            {children}
        </BaseTabs.Tab>
    );
}

export interface TabsPanelProps extends SpacingProps {
    value: string;
    children?: ReactNode;
    className?: string;
}

function TabsPanel({ value, children, className, ...rest }: TabsPanelProps) {
    return (
        <BaseTabs.Panel
            value={value}
            className={[styles.panel, className].filter(Boolean).join(" ")}
            style={{ ...getSpacingStyles(rest) }}
            {...cleanLayoutProps(rest)}
        >
            {children}
        </BaseTabs.Panel>
    );
}

export const Tabs = Object.assign(TabsRoot, {
    List: TabsList,
    Tab: TabsTab,
    Panel: TabsPanel,
});
