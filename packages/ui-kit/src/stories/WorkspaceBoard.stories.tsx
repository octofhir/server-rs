import type { Meta, StoryObj } from "@storybook/react-vite";
import { Database, Settings as Gear, Play, Server, Shield, ClipboardList as SquareListUl, Terminal } from "lucide-react";
import { FhirDashboardAside, WorkspaceBoard, type WorkspaceBoardColumn } from "../widgets";

const meta: Meta<typeof WorkspaceBoard> = {
    title: "Widgets/Workspace Board",
    component: WorkspaceBoard,
    tags: ["autodocs"],
    parameters: {
        layout: "fullscreen",
    },
};

export default meta;
type Story = StoryObj<typeof WorkspaceBoard>;

const columns: WorkspaceBoardColumn[] = [
    {
        id: "operate",
        title: "Operate",
        caption: "Runtime workbench",
        items: [
            {
                id: "rest-console",
                title: "REST Console",
                description: "Compose FHIR requests, inspect responses, and replay operational checks.",
                icon: Terminal,
                status: "Ready",
                statusTone: "success",
                meta: [
                    { id: "fhir", label: "FHIR", tone: "neutral" },
                    { id: "http", label: "HTTP", tone: "info" },
                ],
            },
            {
                id: "packages",
                title: "Packages",
                description: "Review loaded implementation guides and package resource definitions.",
                icon: Database,
                status: "Watch",
                statusTone: "warning",
                meta: [{ id: "ig", label: "IG", tone: "neutral" }],
            },
        ],
    },
    {
        id: "build",
        title: "Build",
        caption: "FHIR builders",
        items: [
            {
                id: "resources",
                title: "Resource Browser",
                description: "Search resource types, inspect metadata, and open records by canonical route.",
                icon: SquareListUl,
                status: "Ready",
                statusTone: "success",
                meta: [{ id: "r4", label: "R4", tone: "neutral" }],
            },
        ],
    },
    {
        id: "govern",
        title: "Govern",
        caption: "Access and audit",
        emptyLabel: "No governance work queued",
        items: [],
    },
];

export const ControlPlane: Story = {
    args: {
        eyebrow: "OctoFHIR workspace",
        title: "FHIR Control Plane",
        description:
            "A command workspace for day-to-day server operations, FHIR modeling, and governance.",
        actions: [
            {
                id: "rest",
                label: "REST console",
                icon: <Play width={16} height={16} />,
                view: "action",
            },
            {
                id: "settings",
                label: "Settings",
                icon: <Gear width={16} height={16} />,
            },
        ],
        metrics: [
            {
                id: "health",
                title: "Server status",
                value: "Healthy",
                caption: "All runtime checks passed",
                icon: <Server width={18} height={18} />,
            },
            {
                id: "catalog",
                title: "Resource catalog",
                value: "148",
                caption: "120 FHIR, 18 system, 10 custom",
                icon: <Database width={18} height={18} />,
            },
            {
                id: "features",
                title: "FHIR capabilities",
                value: "5/7",
                caption: "FHIR R4",
                icon: <SquareListUl width={18} height={18} />,
            },
            {
                id: "security",
                title: "Security",
                value: "Auth",
                caption: "Policies and sessions enabled",
                icon: <Shield width={18} height={18} />,
            },
        ],
        columns,
        aside: (
            <FhirDashboardAside
                surface={{
                    fhirCount: 120,
                    systemCount: 18,
                    customCount: 10,
                    healthLabel: "Healthy",
                    healthTone: "success",
                }}
                capabilities={[
                    { id: "sql", label: "SQL", enabled: true },
                    { id: "graphql", label: "GraphQL", enabled: true },
                    { id: "bulk", label: "Bulk", enabled: false },
                    { id: "db", label: "DB", enabled: true },
                    { id: "auth", label: "Auth", enabled: true },
                    { id: "cql", label: "CQL", enabled: false },
                ]}
                checklist={{
                    title: "FHIR UI goals",
                    items: [
                        { id: "resources", label: "FHIR-aware resource primitives" },
                        { id: "forms", label: "Questionnaire and form layer" },
                        { id: "storybook", label: "Storybook as UI contract" },
                    ],
                }}
            />
        ),
    },
    render: (args) => (
        <div style={{ minHeight: "100vh", padding: 24, background: "var(--g-color-base-background)" }}>
            <WorkspaceBoard {...args} />
        </div>
    ),
};
