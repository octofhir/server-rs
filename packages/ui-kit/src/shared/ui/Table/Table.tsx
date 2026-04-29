import React, { forwardRef } from "react";
import { cleanLayoutProps, getSpacingStyles, mapSpaceValue } from "../layout-utils";
import classes from "./Table.module.css";

type Spacing = "xs" | "sm" | "md" | "lg";

export interface TableProps extends React.TableHTMLAttributes<HTMLTableElement> {
    striped?: boolean;
    highlightOnHover?: boolean;
    withTableBorder?: boolean;
    withColumnBorders?: boolean;
    verticalSpacing?: Spacing;
    horizontalSpacing?: Spacing;
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

type SectionProps<T extends HTMLElement> = React.HTMLAttributes<T>;
type CellProps<T extends HTMLElement> = React.HTMLAttributes<T> & {
    colSpan?: number;
    rowSpan?: number;
    w?: number | string;
    p?: number | string;
};

function cx(...items: Array<string | false | undefined>) {
    return items.filter(Boolean).join(" ");
}

function cellStyle<T extends HTMLElement>({ w, p, style }: CellProps<T>) {
    return {
        ...(w !== undefined ? { width: mapSpaceValue(w) } : {}),
        ...(p !== undefined ? { padding: mapSpaceValue(p) } : {}),
        ...style,
    };
}

const TableRoot = forwardRef<HTMLTableElement, TableProps>(
    (
        {
            className,
            striped,
            highlightOnHover,
            withTableBorder,
            withColumnBorders,
            verticalSpacing = "sm",
            horizontalSpacing: _horizontalSpacing,
            style,
            ...props
        },
        ref,
    ) => {
        const cleaned = cleanLayoutProps(props);
        const mergedStyle = { ...getSpacingStyles(props), ...style };

        return (
            <table
                ref={ref}
                className={cx(
                    classes.root,
                    striped && classes.striped,
                    highlightOnHover && classes.highlight,
                    withTableBorder && classes.bordered,
                    withColumnBorders && classes.columnBorders,
                    verticalSpacing === "xs" && classes.spacingXs,
                    verticalSpacing === "md" && classes.spacingMd,
                    verticalSpacing === "lg" && classes.spacingLg,
                    className,
                )}
                style={mergedStyle}
                {...cleaned}
            />
        );
    },
);
TableRoot.displayName = "Table";

const Thead = forwardRef<HTMLTableSectionElement, SectionProps<HTMLTableSectionElement>>((props, ref) => (
    <thead ref={ref} {...props} />
));
Thead.displayName = "Table.Thead";

const Tbody = forwardRef<HTMLTableSectionElement, SectionProps<HTMLTableSectionElement>>((props, ref) => (
    <tbody ref={ref} {...props} />
));
Tbody.displayName = "Table.Tbody";

const Tr = forwardRef<HTMLTableRowElement, React.HTMLAttributes<HTMLTableRowElement>>((props, ref) => (
    <tr ref={ref} {...props} />
));
Tr.displayName = "Table.Tr";

const Th = forwardRef<HTMLTableCellElement, CellProps<HTMLTableCellElement>>(({ w, p, style, ...props }, ref) => (
    <th ref={ref} style={cellStyle({ w, p, style })} {...props} />
));
Th.displayName = "Table.Th";

const Td = forwardRef<HTMLTableCellElement, CellProps<HTMLTableCellElement>>(({ w, p, style, ...props }, ref) => (
    <td ref={ref} style={cellStyle({ w, p, style })} {...props} />
));
Td.displayName = "Table.Td";

interface ScrollContainerProps extends React.HTMLAttributes<HTMLDivElement> {
    minWidth?: number | string;
}

const ScrollContainer = forwardRef<HTMLDivElement, ScrollContainerProps>(
    ({ minWidth, style, className, children, ...props }, ref) => (
        <div ref={ref} className={cx(classes.scrollContainer, className)} {...props}>
            <div style={{ minWidth: minWidth ? mapSpaceValue(minWidth) : undefined, ...style }}>
                {children}
            </div>
        </div>
    ),
);
ScrollContainer.displayName = "Table.ScrollContainer";

export const Table = Object.assign(TableRoot, {
    Thead,
    Tbody,
    Tr,
    Th,
    Td,
    ScrollContainer,
});
