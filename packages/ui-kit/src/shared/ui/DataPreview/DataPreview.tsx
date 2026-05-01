import type { ReactNode } from "react";
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
                    {rows.map((row, rowIndex) => (
                        <tr key={getRowKey?.(row, rowIndex) ?? rowIndex}>
                            {columns.map((column) => (
                                <td key={column.id}>{row[column.id] ?? null}</td>
                            ))}
                        </tr>
                    ))}
                </tbody>
            </table>
        </div>
    );
}
