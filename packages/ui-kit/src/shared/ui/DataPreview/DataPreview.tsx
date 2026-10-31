import type { ReactNode } from "react";
import { ContextMenu, type DropdownMenuItems } from "../Menu";
import classes from "./DataPreview.module.css";

export interface DataPreviewColumn {
    id: string;
    label: ReactNode;
    width?: number | string;
}

export type DataPreviewRow = Record<string, ReactNode>;

export interface DataPreviewProps {
    columns: DataPreviewColumn[];
    rows: DataPreviewRow[];
    emptyText?: ReactNode;
    maxHeight?: number | string;
    className?: string;
    getRowKey?: (row: DataPreviewRow, index: number) => string;
    /** Right-click actions for a row (usually the same items as its "…" menu). */
    getRowMenu?: (row: DataPreviewRow, index: number) => DropdownMenuItems;
}

function formatSize(value?: number | string) {
    if (value === undefined) return undefined;
    return typeof value === "number" ? `${value}px` : value;
}

export function DataPreview({
    columns,
    rows,
    emptyText = "No data",
    maxHeight,
    className,
    getRowKey,
    getRowMenu,
}: DataPreviewProps) {
    if (!columns.length || !rows.length) {
        return <div className={classes.empty}>{emptyText}</div>;
    }

    return (
        <div
            className={[classes.root, className].filter(Boolean).join(" ")}
            style={{ maxHeight: formatSize(maxHeight) }}
        >
            <table className={classes.table}>
                <thead>
                    <tr>
                        {columns.map((column) => (
                            <th key={column.id} style={{ width: formatSize(column.width) }}>
                                {column.label}
                            </th>
                        ))}
                    </tr>
                </thead>
                <tbody>
                    {rows.map((row, rowIndex) => {
                        const key = getRowKey?.(row, rowIndex) ?? rowIndex;
                        const tr = (
                            <tr key={key}>
                                {columns.map((column) => (
                                    <td key={column.id}>{row[column.id] ?? null}</td>
                                ))}
                            </tr>
                        );
                        return getRowMenu ? (
                            <ContextMenu key={key} items={getRowMenu(row, rowIndex)}>
                                {tr}
                            </ContextMenu>
                        ) : (
                            tr
                        );
                    })}
                </tbody>
            </table>
        </div>
    );
}
