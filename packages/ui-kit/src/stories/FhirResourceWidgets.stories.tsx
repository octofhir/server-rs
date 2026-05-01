import type { Meta, StoryObj } from "@storybook/react-vite";
import {
    ResourceActivityPanel,
    ResourceBundleListPanel,
    ResourceCatalogPanel,
    ResourceSearchSummary,
    ResourceTypeDirectoryPanel,
} from "../widgets/fhir-resource-widgets";

const meta: Meta = {
    title: "Healthcare/FHIR Resource Widgets",
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

const catalog = [
    {
        id: "patient",
        resourceType: "Patient",
        category: "fhir" as const,
        description: "Demographics and administrative patient data.",
        packageName: "hl7.fhir.r4.core",
        count: 12840,
    },
    {
        id: "observation",
        resourceType: "Observation",
        category: "fhir" as const,
        description: "Measurements, lab results, and clinical findings.",
        packageName: "hl7.fhir.r4.core",
        count: 98512,
    },
    {
        id: "access-policy",
        resourceType: "AccessPolicy",
        category: "system" as const,
        description: "Authorization rules for server operations.",
        packageName: "octofhir-auth",
        count: 34,
    },
];

const resourceTypes = [
    {
        id: "Patient",
        name: "Patient",
        category: "fhir" as const,
        canonical: "http://hl7.org/fhir/StructureDefinition/Patient",
        description: "Demographics and administrative patient data.",
        packageName: "hl7.fhir.r4.core",
        count: 12840,
    },
    {
        id: "Observation",
        name: "Observation",
        category: "fhir" as const,
        canonical: "http://hl7.org/fhir/StructureDefinition/Observation",
        description: "Measurements, lab results, and clinical findings.",
        packageName: "hl7.fhir.r4.core",
        count: 98512,
    },
    {
        id: "AccessPolicy",
        name: "AccessPolicy",
        category: "system" as const,
        canonical: "https://octofhir.dev/fhir/StructureDefinition/AccessPolicy",
        description: "Authorization rules for server operations.",
        packageName: "octofhir-auth",
        count: 34,
    },
];

const bundleResources = [
    {
        id: "pt-1001",
        resourceType: "Patient",
        category: "fhir" as const,
        title: "Patient/pt-1001",
        description: "Active patient record with clinical references.",
        versionId: "7",
        lastUpdated: "2026-04-30T10:15:00Z",
        status: "active",
        profileCount: 2,
    },
    {
        id: "pt-1002",
        resourceType: "Patient",
        category: "fhir" as const,
        title: "Patient/pt-1002",
        description: "Administrative demographics imported from package seed data.",
        versionId: "2",
        lastUpdated: "2026-04-29T16:21:00Z",
        status: "active",
        profileCount: 1,
    },
];

const activity = [
    {
        id: "a1",
        resourceType: "Patient",
        resourceId: "pt-1001",
        category: "fhir" as const,
        action: "Updated",
        actor: "admin@example.org",
        occurredAt: "2026-04-30T10:15:00Z",
        tone: "success" as const,
        href: "/ui/resources/Patient/pt-1001",
    },
    {
        id: "a2",
        resourceType: "AccessPolicy",
        resourceId: "policy-7",
        category: "system" as const,
        action: "Changed",
        actor: "security@example.org",
        occurredAt: "2026-04-30T09:48:00Z",
        tone: "warning" as const,
        href: "/ui/auth/policies",
    },
    {
        id: "a3",
        resourceType: "Observation",
        resourceId: "obs-2004",
        category: "fhir" as const,
        action: "Created",
        actor: "import-job",
        occurredAt: "2026-04-30T09:10:00Z",
        tone: "info" as const,
        href: "/ui/resources/Observation/obs-2004",
    },
];

export const ResourceBrowserStack: Story = {
    render: () => (
        <div style={{ display: "grid", gap: 12, maxWidth: 980 }}>
            <ResourceSearchSummary
                resourceType="Patient"
                category="fhir"
                total={12840}
                queryLabel="family=Smith&_count=20"
                facets={[
                    { id: "active", label: "Active", value: 11808 },
                    { id: "inactive", label: "Inactive", value: 1032 },
                ]}
            />

            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 360px", gap: 12 }}>
                <ResourceCatalogPanel
                    description="Most used resource types in this server."
                    resources={catalog}
                />
                <ResourceActivityPanel items={activity} />
            </div>
        </div>
    ),
};

export const ResourceBrowserDirectory: Story = {
    render: () => (
        <div style={{ display: "grid", gridTemplateColumns: "minmax(320px, 0.9fr) minmax(0, 1.1fr)", gap: 12, maxWidth: 1180 }}>
            <ResourceTypeDirectoryPanel
                resources={resourceTypes}
                selectedResourceType="Patient"
            />
            <ResourceBundleListPanel
                resourceType="Patient"
                total={12840}
                items={bundleResources}
                selectedResourceId="pt-1001"
                hasNextPage
                onPreviousPage={() => undefined}
                onNextPage={() => undefined}
            />
        </div>
    ),
};

export const EmptyStates: Story = {
    render: () => (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 12 }}>
            <ResourceCatalogPanel resources={[]} />
            <ResourceActivityPanel items={[]} />
            <ResourceTypeDirectoryPanel resources={[]} />
            <ResourceBundleListPanel items={[]} />
        </div>
    ),
};
