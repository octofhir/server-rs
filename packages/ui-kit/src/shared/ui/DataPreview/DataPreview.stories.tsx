import type { Meta, StoryObj } from "@storybook/react-vite";
import { DataPreview } from "./DataPreview";

const meta: Meta<typeof DataPreview> = {
    title: "Shared/DataPreview",
    component: DataPreview,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof DataPreview>;

export const ResultPreview: Story = {
    args: {
        maxHeight: 240,
        columns: [
            { id: "id", label: "id", width: 120 },
            { id: "resourceType", label: "resourceType", width: 140 },
            { id: "version", label: "version" },
            { id: "updatedAt", label: "updatedAt", width: 220 },
        ],
        rows: [
            {
                id: "pt-1001",
                resourceType: "Patient",
                version: "7",
                updatedAt: "2026-04-30T10:15:00Z",
            },
            {
                id: "pt-1002",
                resourceType: "Patient",
                version: "2",
                updatedAt: "2026-04-29T16:21:00Z",
            },
            {
                id: "obs-2004",
                resourceType: "Observation",
                version: "1",
                updatedAt: "2026-04-30T09:10:00Z",
            },
        ],
    },
};

export const Empty: Story = {
    args: {
        columns: [],
        rows: [],
    },
};
