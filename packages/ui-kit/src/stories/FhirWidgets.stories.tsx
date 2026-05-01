import type { Meta, StoryObj } from "@storybook/react-vite";
import {
    CapabilityFlagsPanel,
    ChecklistPanel,
    FhirDashboardAside,
    FhirSurfacePanel,
} from "../widgets/fhir-dashboard-panels";

const meta: Meta = {
    title: "Healthcare/FHIR Widgets",
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

const capabilities = [
    { id: "sql", label: "SQL", enabled: true },
    { id: "graphql", label: "GraphQL", enabled: true },
    { id: "bulk", label: "Bulk", enabled: false },
    { id: "auth", label: "Auth", enabled: true },
];

export const DashboardAside: Story = {
    render: () => (
        <div style={{ width: 340 }}>
            <FhirDashboardAside
                surface={{
                    fhirCount: 120,
                    systemCount: 18,
                    customCount: 10,
                    healthLabel: "Healthy",
                    healthTone: "success",
                }}
                capabilities={capabilities}
                checklist={{
                    title: "FHIR UI goals",
                    items: [
                        {
                            id: "resources",
                            label: "FHIR-aware resource primitives",
                            description: "Canonical, reference, metadata, and summary surfaces",
                        },
                        {
                            id: "forms",
                            label: "Questionnaire and form layer",
                            description: "Typed clinical form primitives",
                        },
                        {
                            id: "storybook",
                            label: "Storybook as UI contract",
                            description: "Regression stories for every new panel",
                        },
                    ],
                }}
            />
        </div>
    ),
};

export const IndividualPanels: Story = {
    render: () => (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12 }}>
            <FhirSurfacePanel
                fhirCount={120}
                systemCount={18}
                customCount={10}
                healthLabel="Healthy"
                healthTone="success"
            />
            <CapabilityFlagsPanel capabilities={capabilities} />
            <ChecklistPanel
                title="FHIR UI goals"
                items={[
                    {
                        id: "resources",
                        label: "FHIR-aware resource primitives",
                        description: "Canonical, reference, metadata, and summary surfaces",
                    },
                    {
                        id: "forms",
                        label: "Questionnaire and form layer",
                        description: "Typed clinical form primitives",
                    },
                    {
                        id: "storybook",
                        label: "Storybook as UI contract",
                        description: "Regression stories for every new panel",
                    },
                ]}
            />
        </div>
    ),
};
