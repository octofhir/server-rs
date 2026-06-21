import type { Meta, StoryObj } from "@storybook/react-vite";
import { Database, Server, ClipboardList as SquareListUl } from "lucide-react";
import { KeyValueList, MetricTile, PageHeader, StatusBadge, Surface } from "../shared/ui";
import {
    CanonicalUri,
    CapabilityFlag,
    CodingBadge,
    IdentifierBadge,
    OperationOutcomePanel,
    ReferenceLink,
    ResourceName,
    ResourceMetaBar,
    ResourceSummaryCard,
    ResourceTypeBadge,
} from "../shared/fhir";

const meta: Meta = {
    title: "Healthcare/FHIR Primitives",
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

export const WorkspaceFoundations: Story = {
    render: () => (
        <div style={{ display: "grid", gap: 20, maxWidth: 960 }}>
            <PageHeader
                eyebrow="FHIR workspace"
                title="Resource operations"
                description="Shared primitives for FHIR-aware pages, widgets, and entity views."
                actions={[
                    {
                        id: "primary",
                        label: "Run search",
                        view: "action",
                    },
                    {
                        id: "secondary",
                        label: "Configure",
                    },
                ]}
            />

            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12 }}>
                <MetricTile
                    title="Server status"
                    value="Healthy"
                    caption="FHIR API is responding"
                    icon={<Server width={18} height={18} />}
                />
                <MetricTile
                    title="Resource catalog"
                    value="148"
                    caption="120 FHIR, 18 system, 10 custom"
                    icon={<Database width={18} height={18} />}
                />
                <MetricTile
                    title="Capabilities"
                    value="5/6"
                    caption="FHIR R4B"
                    icon={<SquareListUl width={18} height={18} />}
                />
            </div>

            <Surface style={{ display: "grid", gap: 12 }}>
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                    <StatusBadge tone="success">Ready</StatusBadge>
                    <StatusBadge tone="warning">Watch</StatusBadge>
                    <StatusBadge tone="danger">Down</StatusBadge>
                    <ResourceTypeBadge resourceType="Patient" category="fhir" />
                    <ResourceTypeBadge resourceType="AccessPolicy" category="system" />
                    <ResourceTypeBadge resourceType="CareWorkflow" category="custom" />
                </div>

                <CanonicalUri value="http://hl7.org/fhir/StructureDefinition/Patient" />
                <ReferenceLink href="/ui/resources/Patient/example" reference="Patient/example" />
                <div style={{ display: "grid", gap: 8 }}>
                    <ResourceName
                        showType
                        resource={{
                            resourceType: "Patient",
                            id: "example",
                            name: [{ given: ["Alex"], family: "Streltsov" }],
                        }}
                    />
                    <ResourceName
                        showType
                        resource={{
                            resourceType: "StructureDefinition",
                            id: "octofhir-access-policy",
                            title: "Access policy profile",
                        }}
                    />
                </div>
            </Surface>

            <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 320px", gap: 12 }}>
                <ResourceSummaryCard
                    resourceType="StructureDefinition"
                    title="Patient profile"
                    description="Core patient resource definition from the FHIR package catalog."
                    canonical="http://hl7.org/fhir/StructureDefinition/Patient"
                    category="fhir"
                    meta={[
                        { id: "version", label: "R4B", tone: "info" },
                        { id: "status", label: "Active", tone: "success" },
                    ]}
                    id="Patient"
                    versionId="4.3.0"
                    lastUpdated="2026-04-30T10:15:00Z"
                    profileCount={2}
                />

                <Surface style={{ display: "grid", gap: 12 }}>
                    <KeyValueList
                        items={[
                            { id: "fhir", label: "FHIR resources", value: 120 },
                            { id: "system", label: "System resources", value: 18 },
                            { id: "custom", label: "Custom resources", value: 10 },
                        ]}
                    />
                    <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
                        <CapabilityFlag label="SQL" enabled />
                        <CapabilityFlag label="CQL" />
                        <IdentifierBadge system="http://hospital.example/mrn" value="MRN-12345" />
                        <CodingBadge
                            system="http://terminology.hl7.org/CodeSystem/v3-ActCode"
                            code="AMB"
                            display="Ambulatory"
                        />
                    </div>
                    <ResourceMetaBar id="example" versionId="7" lastUpdated="2026-04-30T10:15:00Z" />
                </Surface>
            </div>

            <OperationOutcomePanel
                outcome={{
                    resourceType: "OperationOutcome",
                    issue: [
                        {
                            severity: "error",
                            code: "value",
                            diagnostics: "Observation.value[x] does not match the profile constraint.",
                            expression: ["Observation.valueQuantity"],
                        },
                        {
                            severity: "warning",
                            code: "business-rule",
                            diagnostics: "CodeableConcept has no preferred terminology binding.",
                            expression: ["Observation.code"],
                        },
                    ],
                }}
            />
        </div>
    ),
};
