import type { Meta, StoryObj } from "@storybook/react-vite";
import {
    AuthSessionListPanel,
    OperationCatalogPanel,
    OperationDetailPanel,
    type OperationCatalogItem,
} from "../widgets/control-plane-widgets";

const meta: Meta = {
    title: "Healthcare/Control Plane Widgets",
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

const operations: OperationCatalogItem[] = [
    {
        id: "fhir.patient.search",
        name: "Search Patient",
        description: "FHIR search endpoint with typed query parameters and bundle pagination.",
        category: "FHIR REST API",
        methods: ["GET"],
        pathPattern: "/fhir/Patient",
        public: false,
        module: "octofhir-server::routes::fhir",
    },
    {
        id: "fhir.bundle.transaction",
        name: "Bundle transaction",
        description: "Applies transaction bundles through the FHIR storage pipeline.",
        category: "FHIR REST API",
        methods: ["POST"],
        pathPattern: "/fhir",
        public: false,
        module: "octofhir-server::routes::fhir",
    },
    {
        id: "auth.session.revoke",
        name: "Revoke session",
        description: "Revokes an AuthSession resource and invalidates its token.",
        category: "Authentication",
        methods: ["POST"],
        pathPattern: "/fhir/AuthSession/{id}/$revoke",
        public: false,
        module: "octofhir-server::routes::auth",
        app: { id: "auth", name: "Auth" },
    },
    {
        id: "system.health",
        name: "Health probe",
        description: "Lightweight health endpoint for readiness checks.",
        category: "System",
        methods: ["GET"],
        pathPattern: "/health",
        public: true,
        module: "octofhir-server::routes::system",
    },
];

const sessions = [
    {
        id: "session-current",
        deviceName: "MacBook Pro",
        browserName: "Chrome",
        ipAddress: "10.10.4.21",
        lastActivityLabel: "Just now",
        expiresLabel: "in 7 days",
        status: "active" as const,
        current: true,
    },
    {
        id: "session-ipad",
        deviceName: "iPad",
        browserName: "Safari",
        ipAddress: "10.10.4.33",
        lastActivityLabel: "18 minutes ago",
        expiresLabel: "in 5 days",
        status: "active" as const,
    },
    {
        id: "session-ci",
        deviceName: "CI service account",
        browserName: "Automation client",
        ipAddress: "172.20.0.18",
        lastActivityLabel: "2 hours ago",
        expiresLabel: "in 12 hours",
        status: "active" as const,
    },
];

export const OperationsWorkspace: Story = {
    render: () => (
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.1fr) minmax(320px, 0.9fr)", gap: 12, maxWidth: 1180 }}>
            <OperationCatalogPanel
                description="Grouped route contracts exposed by this OctoFHIR server."
                operations={operations}
                selectedOperationId="auth.session.revoke"
                onSelectOperation={() => undefined}
                onViewOperation={() => undefined}
            />
            <OperationDetailPanel
                operation={operations[2]}
                description="Selected operation contract."
            />
        </div>
    ),
};

export const SessionsWorkspace: Story = {
    render: () => (
        <div style={{ maxWidth: 880 }}>
            <AuthSessionListPanel
                description="Active AuthSession resources for the current user."
                sessions={sessions}
                selectedSessionId="session-ipad"
                onSelectSession={() => undefined}
                onRevokeSession={() => undefined}
            />
        </div>
    ),
};

export const EmptyStates: Story = {
    render: () => (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12 }}>
            <OperationCatalogPanel operations={[]} />
            <OperationDetailPanel />
            <AuthSessionListPanel sessions={[]} />
        </div>
    ),
};
