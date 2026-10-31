import { type ReactNode, useMemo, useRef, useState } from "react";
import {
    type ColumnDef,
    type ColumnFiltersState,
    type PaginationState,
    type Row,
    type RowSelectionState,
    type SortingState,
    getCoreRowModel,
    getFilteredRowModel,
    getPaginationRowModel,
    getSortedRowModel,
    useReactTable,
} from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, ChevronsUpDown } from "lucide-react";
import { Checkbox } from "../Checkbox";
import type { Size } from "../layout-utils";
import type { DropdownMenuItems } from "../Menu";
import { Skeleton } from "../Skeleton";
import classes from "./DataTable.module.css";
import { RowContextMenu } from "./RowContextMenu";

/** A single column definition. Canonical, framework-agnostic surface over TanStack Table. */
export interface DataTableColumn<T> {
    /** Stable column id (also used for sort/filter state). */
    id: string;
    /** Header content. */
    header: ReactNode;
    /** Extracts the sortable/filterable value for the row. Defaults to `row[id]`. */
    accessor?: (row: T) => unknown;
    /** Custom cell renderer. Defaults to the accessor value rendered as text. */
    cell?: (row: T) => ReactNode;
    /** Enable click-to-sort on the header. */
    sortable?: boolean;
    /** Enable the per-column text filter input. */
    filterable?: boolean;
    /** Fixed column width (px number or CSS length). */
    width?: number | string;
    /** Horizontal text alignment. */
    align?: "left" | "center" | "right";
}

/** Sort descriptor — mirrors TanStack `SortingState` entries. */
export interface DataTableSort {
    id: string;
    desc: boolean;
}

/** Column filter descriptor. */
export interface DataTableFilter {
    id: string;
    value: string;
}

/** Pagination descriptor (zero-based page index). */
export interface DataTablePagination {
    pageIndex: number;
    pageSize: number;
}

export interface DataTableProps<T> {
    /** Row data. In server-driven mode (`rowCount` set) this is the current page only. */
    data: T[];
    columns: DataTableColumn<T>[];
    /** Stable row id. Defaults to the row index. Required for reliable selection. */
    getRowId?: (row: T) => string;

    /** Control height to match neighbouring inputs/buttons. */
    size?: Size;
    striped?: boolean;
    highlightOnHover?: boolean;
    withColumnBorders?: boolean;
    /** Keep the header visible while the body scrolls. */
    stickyHeader?: boolean;
    /** Fill the parent's height: the body scrolls and the pager stays pinned at the bottom. */
    fillHeight?: boolean;

    // --- sorting (controlled or uncontrolled) ---
    sorting?: DataTableSort[];
    onSortingChange?: (sorting: DataTableSort[]) => void;

    // --- column filters (controlled or uncontrolled) ---
    columnFilters?: DataTableFilter[];
    onColumnFiltersChange?: (filters: DataTableFilter[]) => void;

    // --- pagination ---
    /** Enable client-side pagination. Implied when `pagination`/`rowCount` is set. */
    paginated?: boolean;
    /** Page size for uncontrolled pagination. */
    pageSize?: number;
    pagination?: DataTablePagination;
    onPaginationChange?: (pagination: DataTablePagination) => void;
    /**
     * Total row count across all pages. Setting this switches the table to
     * server-driven mode: sorting, filtering and pagination are NOT applied
     * locally — feed `data` for the current page and react to the callbacks.
     */
    rowCount?: number;

    // --- virtualization ---
    virtualized?: boolean;
    estimateRowHeight?: number;
    maxHeight?: number | string;

    // --- selection ---
    selectable?: boolean;
    selectedRowIds?: string[];
    onSelectedRowIdsChange?: (ids: string[]) => void;
    /** The row that is open elsewhere (e.g. in a side panel); highlighted apart from selection. */
    activeRowId?: string;

    // --- states & interaction ---
    loading?: boolean;
    /** Skeleton row count shown while `loading`. */
    loadingRowCount?: number;
    emptyState?: ReactNode;
    onRowClick?: (row: T) => void;
    /** Right-click (or the menu key) on a row opens these actions. */
    rowMenu?: (row: T) => DropdownMenuItems;

    className?: string;
    "aria-label"?: string;
}

function toSortingState(sorts: DataTableSort[] | undefined): SortingState {
    return (sorts ?? []).map((s) => ({ id: s.id, desc: s.desc }));
}

function toFilterState(filters: DataTableFilter[] | undefined): ColumnFiltersState {
    return (filters ?? []).map((f) => ({ id: f.id, value: f.value }));
}

export function DataTable<T>({
    data,
    columns,
    getRowId,
    size = "md",
    striped,
    highlightOnHover = true,
    withColumnBorders,
    stickyHeader,
    fillHeight,
    sorting: sortingProp,
    onSortingChange,
    columnFilters: filtersProp,
    onColumnFiltersChange,
    paginated,
    pageSize = 25,
    pagination: paginationProp,
    onPaginationChange,
    rowCount,
    virtualized,
    estimateRowHeight = 40,
    maxHeight = 480,
    selectable,
    selectedRowIds,
    onSelectedRowIdsChange,
    activeRowId,
    loading,
    loadingRowCount = 8,
    emptyState = "No data",
    onRowClick,
    rowMenu,
    className,
    "aria-label": ariaLabel,
}: DataTableProps<T>) {
    // The row under the pointer is recorded on `contextmenu` before the menu asks to open.
    const pendingMenuRow = useRef<Row<T> | null>(null);
    const [menu, setMenu] = useState<{ rowId: string; items: DropdownMenuItems } | null>(null);
    const onMenuOpenChange = (open: boolean) => {
        const row = pendingMenuRow.current;
        setMenu(open && row && rowMenu ? { rowId: row.id, items: rowMenu(row.original) } : null);
        if (!open) pendingMenuRow.current = null;
    };
    const rowMenuProps = (row: Row<T>) =>
        rowMenu
            ? {
                  menuOpen: menu?.rowId === row.id,
                  onContextMenu: () => {
                      pendingMenuRow.current = row;
                  },
              }
            : {};
    const serverMode = rowCount != null;
    const paginationEnabled = paginated || paginationProp != null || serverMode;

    // --- controlled/uncontrolled state plumbing ---
    const [sortingState, setSortingState] = useState<SortingState>(() => toSortingState(sortingProp));
    const sorting = sortingProp != null ? toSortingState(sortingProp) : sortingState;

    const [filterState, setFilterState] = useState<ColumnFiltersState>(() => toFilterState(filtersProp));
    const columnFilters = filtersProp != null ? toFilterState(filtersProp) : filterState;

    const [paginationState, setPaginationState] = useState<PaginationState>(() => ({
        pageIndex: paginationProp?.pageIndex ?? 0,
        pageSize: paginationProp?.pageSize ?? pageSize,
    }));
    const pagination = paginationProp != null ? paginationProp : paginationState;

    const rowSelection = useMemo<RowSelectionState>(() => {
        if (selectedRowIds == null) return {};
        const next: RowSelectionState = {};
        for (const id of selectedRowIds) next[id] = true;
        return next;
    }, [selectedRowIds]);
    const [internalSelection, setInternalSelection] = useState<RowSelectionState>({});
    const selection = selectedRowIds != null ? rowSelection : internalSelection;

    const tableColumns = useMemo<ColumnDef<T>[]>(() => {
        const cols: ColumnDef<T>[] = [];
        for (const c of columns) {
            cols.push({
                id: c.id,
                accessorFn: c.accessor ?? ((row) => (row as Record<string, unknown>)[c.id]),
                enableSorting: c.sortable ?? false,
                enableColumnFilter: c.filterable ?? false,
                filterFn: "includesString",
                size: typeof c.width === "number" ? c.width : undefined,
            });
        }
        return cols;
    }, [columns]);

    const table = useReactTable<T>({
        data,
        columns: tableColumns,
        state: { sorting, columnFilters, pagination, rowSelection: selection },
        getRowId: getRowId ? (row) => getRowId(row) : undefined,
        manualSorting: serverMode,
        manualFiltering: serverMode,
        manualPagination: serverMode,
        rowCount: serverMode ? rowCount : undefined,
        enableRowSelection: selectable,
        getCoreRowModel: getCoreRowModel(),
        getSortedRowModel: serverMode ? undefined : getSortedRowModel(),
        getFilteredRowModel: serverMode ? undefined : getFilteredRowModel(),
        getPaginationRowModel: serverMode || !paginationEnabled ? undefined : getPaginationRowModel(),
        onSortingChange: (updater) => {
            const next = typeof updater === "function" ? updater(sorting) : updater;
            setSortingState(next);
            onSortingChange?.(next.map((s) => ({ id: s.id, desc: s.desc })));
        },
        onColumnFiltersChange: (updater) => {
            const next = typeof updater === "function" ? updater(columnFilters) : updater;
            setFilterState(next);
            onColumnFiltersChange?.(next.map((f) => ({ id: f.id, value: String(f.value ?? "") })));
        },
        onPaginationChange: (updater) => {
            const next = typeof updater === "function" ? updater(pagination) : updater;
            setPaginationState(next);
            onPaginationChange?.(next);
        },
        onRowSelectionChange: (updater) => {
            const next = typeof updater === "function" ? updater(selection) : updater;
            setInternalSelection(next);
            onSelectedRowIdsChange?.(Object.keys(next).filter((k) => next[k]));
        },
    });

    const rows = table.getRowModel().rows;
    const hasFilterRow = columns.some((c) => c.filterable);
    const colCount = columns.length + (selectable ? 1 : 0);

    // --- virtualization ---
    const scrollRef = useRef<HTMLDivElement>(null);
    const rowVirtualizer = useVirtualizer({
        count: rows.length,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => estimateRowHeight,
        overscan: 12,
        enabled: virtualized,
    });

    const renderHeaderCell = (c: DataTableColumn<T>) => {
        const col = table.getColumn(c.id);
        const sorted = col?.getIsSorted();
        const canSort = c.sortable;
        return (
            <th
                key={c.id}
                className={classes.th}
                style={{ width: c.width, textAlign: c.align }}
                data-sortable={canSort ? "true" : undefined}
                aria-sort={sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : undefined}
            >
                {canSort ? (
                    <button type="button" className={classes.sortButton} onClick={() => col?.toggleSorting()}>
                        <span>{c.header}</span>
                        {sorted === "asc" ? (
                            <ArrowUp size={14} className={classes.sortIcon} />
                        ) : sorted === "desc" ? (
                            <ArrowDown size={14} className={classes.sortIcon} />
                        ) : (
                            <ChevronsUpDown size={14} className={classes.sortIconIdle} />
                        )}
                    </button>
                ) : (
                    <span className={classes.headerLabel}>{c.header}</span>
                )}
            </th>
        );
    };

    const renderFilterCell = (c: DataTableColumn<T>) => {
        const col = table.getColumn(c.id);
        return (
            <th key={c.id} className={classes.filterTh}>
                {c.filterable ? (
                    <input
                        className={classes.filterInput}
                        value={(col?.getFilterValue() as string) ?? ""}
                        placeholder="Filter…"
                        onChange={(e) => col?.setFilterValue(e.target.value)}
                        aria-label={`Filter ${typeof c.header === "string" ? c.header : c.id}`}
                    />
                ) : null}
            </th>
        );
    };

    const renderDataCells = (row: Row<T>) =>
        columns.map((c) => {
            const content = c.cell ? c.cell(row.original) : (renderValue(row.getValue(c.id)) as ReactNode);
            return (
                <td key={c.id} className={classes.td} style={{ width: c.width, textAlign: c.align }}>
                    {content}
                </td>
            );
        });

    const selectHeaderCell = selectable ? (
        <th className={classes.selectCell}>
            <Checkbox
                checked={table.getIsAllRowsSelected()}
                indeterminate={table.getIsSomeRowsSelected()}
                onChange={(checked) => table.toggleAllRowsSelected(checked)}
                aria-label="Select all rows"
            />
        </th>
    ) : null;

    const selectBodyCell = (row: Row<T>) =>
        selectable ? (
            <td className={classes.selectCell} onClick={(e) => e.stopPropagation()}>
                <Checkbox
                    checked={row.getIsSelected()}
                    disabled={!row.getCanSelect()}
                    onChange={(checked) => row.toggleSelected(checked)}
                    aria-label="Select row"
                />
            </td>
        ) : null;

    const showEmpty = !loading && rows.length === 0;
    const virtualItems = virtualized ? rowVirtualizer.getVirtualItems() : null;
    const padTop = virtualItems?.length ? virtualItems[0].start : 0;
    const padBottom = virtualItems?.length
        ? rowVirtualizer.getTotalSize() - virtualItems[virtualItems.length - 1].end
        : 0;
    const loadingKeys = useMemo(
        () => Array.from({ length: loadingRowCount }, (_, i) => `dt-skeleton-${i}`),
        [loadingRowCount],
    );

    return (
        <div
            className={[classes.root, fillHeight && classes.rootFill, className]
                .filter(Boolean)
                .join(" ")}
        >
            <div
                ref={scrollRef}
                className={[classes.scroll, fillHeight && classes.scrollFill]
                    .filter(Boolean)
                    .join(" ")}
                style={{
                    maxHeight: fillHeight ? undefined : virtualized || stickyHeader ? maxHeight : undefined,
                }}
            >
                <TableMenuArea
                    enabled={rowMenu != null}
                    items={menu?.items ?? null}
                    open={menu != null}
                    onOpenChange={onMenuOpenChange}
                >
                    <table
                        className={[
                            classes.table,
                            striped && classes.striped,
                            highlightOnHover && classes.highlight,
                            withColumnBorders && classes.columnBorders,
                            stickyHeader && classes.stickyHeader,
                            onRowClick && classes.clickable,
                        ]
                            .filter(Boolean)
                            .join(" ")}
                        data-size={size}
                        data-has-selection={Object.values(selection).some(Boolean) ? "true" : undefined}
                        aria-label={ariaLabel}
                    >
                        <thead className={classes.thead}>
                            <tr>
                                {selectHeaderCell}
                                {columns.map(renderHeaderCell)}
                            </tr>
                            {hasFilterRow && (
                                <tr className={classes.filterRow}>
                                    {selectable && <th className={classes.selectCell} />}
                                    {columns.map(renderFilterCell)}
                                </tr>
                            )}
                        </thead>
                        <tbody className={classes.tbody}>
                            {loading
                                ? loadingKeys.map((key) => (
                                      <tr key={key} className={classes.tr}>
                                          {selectable && (
                                              <td className={classes.selectCell}>
                                                  <Skeleton w={16} h={16} radius={4} />
                                              </td>
                                          )}
                                          {columns.map((c) => (
                                              <td key={c.id} className={classes.td}>
                                                  <Skeleton h="0.9em" />
                                              </td>
                                          ))}
                                      </tr>
                                  ))
                                : showEmpty
                                  ? (
                                      <tr>
                                          <td className={classes.emptyCell} colSpan={colCount}>
                                              {emptyState}
                                          </td>
                                      </tr>
                                  )
                                  : virtualized && virtualItems
                                    ? (
                                          <>
                                              {padTop > 0 && (
                                                  <tr className={classes.spacerRow}>
                                                      <td colSpan={colCount} style={{ height: padTop, padding: 0, border: 0 }} />
                                                  </tr>
                                              )}
                                              {virtualItems.map((vi) => {
                                                  const row = rows[vi.index];
                                                  return (
                                                      <DataTableRow
                                                          key={row.id}
                                                          row={row}
                                                          measureRef={rowVirtualizer.measureElement}
                                                          index={vi.index}
                                                          selectCell={selectBodyCell(row)}
                                                          onRowClick={onRowClick}
                                                          selectable={selectable}
                                                          active={row.id === activeRowId}
                                                          {...rowMenuProps(row)}
                                                      >
                                                          {renderDataCells(row)}
                                                      </DataTableRow>
                                                  );
                                              })}
                                              {padBottom > 0 && (
                                                  <tr className={classes.spacerRow}>
                                                      <td colSpan={colCount} style={{ height: padBottom, padding: 0, border: 0 }} />
                                                  </tr>
                                              )}
                                          </>
                                      )
                                    : rows.map((row) => (
                                          <DataTableRow
                                              key={row.id}
                                              row={row}
                                              selectCell={selectBodyCell(row)}
                                              onRowClick={onRowClick}
                                              selectable={selectable}
                                              active={row.id === activeRowId}
                                              {...rowMenuProps(row)}
                                          >
                                              {renderDataCells(row)}
                                          </DataTableRow>
                                      ))}
                        </tbody>
                    </table>
                </TableMenuArea>
            </div>
            {paginationEnabled && !showEmpty && (
                <DataTablePager
                    pageIndex={pagination.pageIndex}
                    pageSize={pagination.pageSize}
                    pageCount={table.getPageCount()}
                    canPrevious={table.getCanPreviousPage()}
                    canNext={table.getCanNextPage()}
                    rowCount={serverMode ? rowCount : rows.length}
                    onPrevious={() => table.previousPage()}
                    onNext={() => table.nextPage()}
                />
            )}
        </div>
    );
}

function TableMenuArea({
    enabled,
    children,
    ...menu
}: {
    enabled: boolean;
    children: ReactNode;
    items: DropdownMenuItems | null;
    open: boolean;
    onOpenChange: (open: boolean) => void;
}) {
    return enabled ? <RowContextMenu {...menu}>{children}</RowContextMenu> : <>{children}</>;
}

interface DataTableRowProps<T> {
    row: Row<T>;
    children: ReactNode;
    selectCell: ReactNode;
    onRowClick?: (row: T) => void;
    selectable?: boolean;
    active?: boolean;
    menuOpen?: boolean;
    onContextMenu?: () => void;
    measureRef?: (node: Element | null) => void;
    index?: number;
}

function DataTableRow<T>({
    row,
    children,
    selectCell,
    onRowClick,
    selectable,
    active,
    menuOpen,
    onContextMenu,
    measureRef,
    index,
}: DataTableRowProps<T>) {
    const clickable = onRowClick != null;
    const focusable = clickable || selectable || onContextMenu != null;
    return (
        <tr
            ref={measureRef}
            data-index={index}
            className={classes.tr}
            data-selected={row.getIsSelected() ? "true" : undefined}
            data-active={active ? "true" : undefined}
            data-menu-open={menuOpen ? "true" : undefined}
            tabIndex={focusable ? 0 : undefined}
            onClick={clickable ? () => onRowClick(row.original) : undefined}
            onContextMenu={onContextMenu}
            onKeyDown={
                focusable
                    ? (e) => {
                          if (clickable && (e.key === "Enter" || e.key === " ")) {
                              e.preventDefault();
                              onRowClick(row.original);
                              return;
                          }
                          // Shift+F10 is the keyboard right-click; browsers on macOS don't map it.
                          if (onContextMenu && e.key === "F10" && e.shiftKey) {
                              e.preventDefault();
                              const rect = e.currentTarget.getBoundingClientRect();
                              e.currentTarget.dispatchEvent(
                                  new MouseEvent("contextmenu", {
                                      bubbles: true,
                                      cancelable: true,
                                      clientX: rect.left + 24,
                                      clientY: rect.bottom,
                                  }),
                              );
                              return;
                          }
                          // Linear-style: x toggles the focused row's selection.
                          if (selectable && e.key === "x" && row.getCanSelect()) {
                              e.preventDefault();
                              row.toggleSelected();
                              return;
                          }
                          // Keyboard-first lists: ↑/↓ or k/j move between rows.
                          const step =
                              e.key === "ArrowDown" || e.key === "j" ? 1 : e.key === "ArrowUp" || e.key === "k" ? -1 : 0;
                          if (step === 0) return;
                          const sibling = step > 0 ? e.currentTarget.nextElementSibling : e.currentTarget.previousElementSibling;
                          if (sibling instanceof HTMLElement) {
                              e.preventDefault();
                              sibling.focus();
                          }
                      }
                    : undefined
            }
        >
            {selectCell}
            {children}
        </tr>
    );
}

function renderValue(value: unknown): ReactNode {
    if (value == null) return null;
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
}

interface PagerProps {
    pageIndex: number;
    pageSize: number;
    pageCount: number;
    canPrevious: boolean;
    canNext: boolean;
    rowCount: number;
    onPrevious: () => void;
    onNext: () => void;
}

function DataTablePager({ pageIndex, pageSize, pageCount, canPrevious, canNext, rowCount, onPrevious, onNext }: PagerProps) {
    const from = rowCount === 0 ? 0 : pageIndex * pageSize + 1;
    const to = Math.min((pageIndex + 1) * pageSize, rowCount);
    return (
        <div className={classes.pager}>
            <span className={classes.pagerInfo}>
                {from}–{to} of {rowCount}
            </span>
            <div className={classes.pagerButtons}>
                <button type="button" className={classes.pagerButton} onClick={onPrevious} disabled={!canPrevious}>
                    Previous
                </button>
                <span className={classes.pagerPage}>
                    {pageIndex + 1} / {Math.max(pageCount, 1)}
                </span>
                <button type="button" className={classes.pagerButton} onClick={onNext} disabled={!canNext}>
                    Next
                </button>
            </div>
        </div>
    );
}
