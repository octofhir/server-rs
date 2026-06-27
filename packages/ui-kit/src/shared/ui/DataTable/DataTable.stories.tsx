import { useMemo, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Badge } from "../Badge";
import { DataTable, type DataTableColumn, type DataTablePagination, type DataTableSort } from "./DataTable";

interface Patient {
    id: string;
    name: string;
    gender: "male" | "female" | "other";
    birthDate: string;
    active: boolean;
}

const GENDERS = ["male", "female", "other"] as const;

function makePatients(n: number): Patient[] {
    return Array.from({ length: n }, (_, i) => ({
        id: `pat-${i + 1}`,
        name: `Patient ${String(i + 1).padStart(4, "0")}`,
        gender: GENDERS[i % 3],
        birthDate: `19${50 + (i % 50)}-0${(i % 9) + 1}-1${i % 9}`,
        active: i % 4 !== 0,
    }));
}

const columns: DataTableColumn<Patient>[] = [
    { id: "id", header: "ID", width: 120 },
    { id: "name", header: "Name", sortable: true, filterable: true },
    { id: "gender", header: "Gender", sortable: true, filterable: true, width: 120 },
    { id: "birthDate", header: "Birth date", sortable: true, width: 140 },
    {
        id: "active",
        header: "Status",
        width: 120,
        cell: (p) => (
            <Badge color={p.active ? "green" : "gray"} variant="light">
                {p.active ? "Active" : "Inactive"}
            </Badge>
        ),
    },
];

const meta: Meta<typeof DataTable<Patient>> = {
    title: "Data display/DataTable",
    component: DataTable,
    tags: ["autodocs"],
    parameters: { layout: "padded" },
};
export default meta;
type Story = StoryObj<typeof DataTable<Patient>>;

export const Basic: Story = {
    args: {
        data: makePatients(8),
        columns,
        getRowId: (p) => p.id,
    },
};

export const SortableFilterable: Story = {
    args: {
        data: makePatients(50),
        columns,
        getRowId: (p) => p.id,
        striped: true,
        paginated: true,
        pageSize: 10,
    },
};

export const Selectable: Story = {
    render: (args) => {
        const [selected, setSelected] = useState<string[]>(["pat-2"]);
        return (
            <div>
                <p style={{ fontSize: 13, marginBottom: 8 }}>Selected: {selected.join(", ") || "none"}</p>
                <DataTable
                    {...args}
                    selectable
                    selectedRowIds={selected}
                    onSelectedRowIdsChange={setSelected}
                />
            </div>
        );
    },
    args: {
        data: makePatients(8),
        columns,
        getRowId: (p) => p.id,
        highlightOnHover: true,
    },
};

export const Virtualized: Story = {
    args: {
        data: makePatients(10000),
        columns,
        getRowId: (p) => p.id,
        virtualized: true,
        maxHeight: 420,
        stickyHeader: true,
    },
};

export const Loading: Story = {
    args: {
        data: [],
        columns,
        loading: true,
        loadingRowCount: 6,
    },
};

export const Empty: Story = {
    args: {
        data: [],
        columns,
        emptyState: "No patients found",
    },
};

export const ServerDriven: Story = {
    render: (args) => {
        const all = useMemo(() => makePatients(137), []);
        const [pagination, setPagination] = useState<DataTablePagination>({ pageIndex: 0, pageSize: 10 });
        const [sorting, setSorting] = useState<DataTableSort[]>([]);

        const sorted = useMemo(() => {
            if (!sorting.length) return all;
            const { id, desc } = sorting[0];
            return [...all].sort((a, b) => {
                const av = String(a[id as keyof Patient]);
                const bv = String(b[id as keyof Patient]);
                return desc ? bv.localeCompare(av) : av.localeCompare(bv);
            });
        }, [all, sorting]);

        const page = sorted.slice(
            pagination.pageIndex * pagination.pageSize,
            (pagination.pageIndex + 1) * pagination.pageSize,
        );

        return (
            <DataTable
                {...args}
                data={page}
                rowCount={all.length}
                pagination={pagination}
                onPaginationChange={setPagination}
                sorting={sorting}
                onSortingChange={setSorting}
            />
        );
    },
    args: {
        columns,
        getRowId: (p) => p.id,
        data: [],
    },
};
