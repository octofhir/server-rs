import type { Meta, StoryObj } from "@storybook/react-vite";
import {
    FhirPackageListPanel,
    FhirPackageRegistryPanel,
    FhirPackageResourceTypesPanel,
} from "../widgets/fhir-package-widgets";

const meta: Meta = {
    title: "Healthcare/FHIR Package Widgets",
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

const installedPackages = [
    {
        name: "hl7.fhir.r4.core",
        version: "4.0.1",
        fhirVersion: "4.0.1",
        resourceCount: 146,
        installedAt: "2026-04-30",
        description: "Core R4 StructureDefinitions, ValueSets, CodeSystems, and search parameters.",
        isCompatible: true,
    },
    {
        name: "hl7.fhir.us.core",
        version: "6.1.0",
        fhirVersion: "4.0.1",
        resourceCount: 312,
        installedAt: "2026-04-28",
        description: "US Core implementation guide artifacts for clinical data exchange.",
        isCompatible: true,
    },
    {
        name: "hl7.fhir.uv.ips",
        version: "2.0.0-ballot",
        fhirVersion: "5.0.0",
        resourceCount: 88,
        installedAt: "2026-04-21",
        description: "International Patient Summary ballot package staged for compatibility review.",
        isCompatible: false,
    },
];

const registryResults = [
    {
        name: "hl7.fhir.us.core",
        latestVersion: "7.0.0",
        versions: ["7.0.0", "6.1.0", "6.0.0", "5.0.1", "4.1.0"],
        installedVersions: ["6.1.0"],
        description: "US Core implementation guide package.",
    },
    {
        name: "hl7.fhir.uv.ips",
        latestVersion: "2.0.0",
        versions: ["2.0.0", "1.1.0", "1.0.0"],
        description: "International Patient Summary implementation guide.",
    },
];

const resourceTypes = [
    { resourceType: "StructureDefinition", count: 184, tone: "info" as const },
    { resourceType: "ValueSet", count: 62, tone: "success" as const },
    { resourceType: "CodeSystem", count: 21, tone: "warning" as const },
    { resourceType: "SearchParameter", count: 45, tone: "neutral" as const },
];

export const PackageWorkspace: Story = {
    render: () => (
        <div style={{ display: "grid", gap: 12, maxWidth: 1180 }}>
            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1.1fr) minmax(320px, 0.9fr)", gap: 12 }}>
                <FhirPackageListPanel
                    description="Packages currently loaded into this FHIR control plane."
                    packages={installedPackages}
                    selectedPackageId="hl7.fhir.r4.core@4.0.1"
                    onSelectPackage={() => undefined}
                    onViewPackage={() => undefined}
                />
                <FhirPackageResourceTypesPanel
                    description="Artifact distribution inside the selected package."
                    resourceTypes={resourceTypes}
                    selectedResourceType="StructureDefinition"
                    onSelectResourceType={() => undefined}
                />
            </div>
            <FhirPackageRegistryPanel
                description="Registry lookup results with installed versions disabled."
                results={registryResults}
                selectedPackageName="hl7.fhir.us.core"
                selectedVersion="7.0.0"
                onSelectPackage={() => undefined}
                onInstallPackage={() => undefined}
            />
        </div>
    ),
};

export const EmptyStates: Story = {
    render: () => (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12 }}>
            <FhirPackageListPanel packages={[]} />
            <FhirPackageRegistryPanel results={[]} />
            <FhirPackageResourceTypesPanel resourceTypes={[]} />
        </div>
    ),
};
