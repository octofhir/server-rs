import type { Meta, StoryObj } from "@storybook/react-vite";
import { StatGrid } from "./StatGrid";

const meta: Meta<typeof StatGrid> = {
    title: "Shared/StatGrid",
    component: StatGrid,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof StatGrid>;

export const ClinicalSurface: Story = {
    args: {
        items: [
            {
                id: "fhir",
                label: "FHIR resources",
                value: "120",
                caption: "Core definitions",
                tone: "success",
            },
            {
                id: "system",
                label: "System",
                value: "18",
                caption: "Control plane",
                tone: "info",
            },
            {
                id: "custom",
                label: "Custom",
                value: "10",
                caption: "Local extensions",
                tone: "warning",
            },
        ],
    },
    render: (args) => (
        <div style={{ maxWidth: 520 }}>
            <StatGrid {...args} />
        </div>
    ),
};
