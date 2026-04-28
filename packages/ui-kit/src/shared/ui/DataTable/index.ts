/**
 * Re-exports of `@gravity-ui/table` — TanStack-Table-driven data table for FHIR
 * resources, search results, etc. Prefer `Table` from `@gravity-ui/table` for
 * data grids and the simpler `Table` component from `@gravity-ui/uikit` for
 * static markup.
 */
export {
    BaseTable,
    Table as DataTable,
    TableSettings,
    SortIndicator,
    useTable,
    useTableSettings,
    selectionColumn,
    dragHandleColumn,
    getActionsColumn,
    getSettingsColumn,
} from "@gravity-ui/table";
export type {
    BaseTableProps,
    TableProps as DataTableProps,
    TableSettingsProps,
    TableSettingsOptions,
    UseTableSettingsOptions,
} from "@gravity-ui/table";
