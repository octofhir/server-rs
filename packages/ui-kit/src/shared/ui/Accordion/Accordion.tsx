import type { ReactNode } from "react";
import { Accordion as BaseAccordion } from "@base-ui/react/accordion";
import { ChevronDown } from "lucide-react";
import styles from "./Accordion.module.css";

export interface AccordionProps {
    value?: string[];
    defaultValue?: string[];
    onValueChange?: (value: string[]) => void;
    /** Allow more than one item open at a time. */
    openMultiple?: boolean;
    disabled?: boolean;
    className?: string;
    style?: React.CSSProperties;
    children?: ReactNode;
}

export interface AccordionItemProps {
    /** Stable identifier used to control open state. */
    value: string;
    title: ReactNode;
    disabled?: boolean;
    className?: string;
    children?: ReactNode;
}

function AccordionItem({ value, title, disabled, className, children }: AccordionItemProps) {
    return (
        <BaseAccordion.Item
            value={value}
            disabled={disabled}
            className={[styles.item, className].filter(Boolean).join(" ")}
        >
            <BaseAccordion.Header className={styles.header}>
                <BaseAccordion.Trigger className={styles.trigger}>
                    {title}
                    <ChevronDown size={16} className={styles.chevron} />
                </BaseAccordion.Trigger>
            </BaseAccordion.Header>
            <BaseAccordion.Panel className={styles.panel}>
                <div className={styles.panelInner}>{children}</div>
            </BaseAccordion.Panel>
        </BaseAccordion.Item>
    );
}

export function Accordion({
    value,
    defaultValue,
    onValueChange,
    openMultiple = false,
    disabled,
    className,
    style,
    children,
}: AccordionProps) {
    return (
        <BaseAccordion.Root
            value={value}
            defaultValue={defaultValue}
            onValueChange={onValueChange}
            multiple={openMultiple}
            disabled={disabled}
            className={[styles.root, className].filter(Boolean).join(" ")}
            style={style}
        >
            {children}
        </BaseAccordion.Root>
    );
}

Accordion.Item = AccordionItem;
