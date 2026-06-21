import type { Meta, StoryObj } from "@storybook/react-vite";
import { Database, Play, Server } from "lucide-react";
import { UIProvider, type OctoThemeConfig } from "../index";
import {
    Button,
    CommandCard,
    MetricTile,
    PageHeader,
    SectionPanel,
    StatusBadge,
} from "../shared/ui";
import { CapabilityFlag, ResourceTypeBadge } from "../shared/fhir";

const meta: Meta = {
    title: "Foundations/Theme Overrides",
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

const clinicalTheme: OctoThemeConfig = {
    tokens: {
        brand: {
            primary: "oklch(58% 0.16 185)",
            primaryHover: "oklch(50% 0.15 185)",
            primaryActive: "oklch(42% 0.14 185)",
            primarySoft: "oklch(96% 0.035 185)",
            primaryRing: "oklch(84% 0.08 185)",
            info: "oklch(58% 0.15 230)",
            infoHover: "oklch(50% 0.14 230)",
            infoSoft: "oklch(95% 0.035 230)",
        },
        scheme: {
            light: {
                accent: {
                    primary: "oklch(58% 0.16 185)",
                    primaryBg: "oklch(96% 0.035 185)",
                    primaryBgHover: "oklch(92% 0.055 185)",
                },
            },
            dark: {
                accent: {
                    primary: "oklch(72% 0.13 185)",
                    primaryBg: "oklch(26% 0.045 185)",
                    primaryBgHover: "oklch(32% 0.055 185)",
                },
            },
        },
    },
    cssVariables: {
        "--demo-theme-ring": "oklch(58% 0.16 185 / 0.32)",
    },
};

export const ProviderThemeOverride: Story = {
    render: () => (
        <UIProvider defaultColorScheme="light" theme={clinicalTheme}>
            <div style={{ display: "grid", gap: 20, maxWidth: 920 }}>
                <PageHeader
                    eyebrow="Theme API"
                    title="Clinical workspace theme"
                    description="The UIProvider keeps OctoFHIR defaults and merges product-level overrides through props."
                    actions={[
                        {
                            id: "run",
                            label: "Run",
                            icon: <Play width={16} height={16} />,
                            view: "action",
                        },
                    ]}
                />

                <div style={{ display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 12 }}>
                    <MetricTile
                        title="FHIR API"
                        value="Healthy"
                        caption="The brand color comes from UIProvider props"
                        icon={<Server width={18} height={18} />}
                    />
                    <MetricTile
                        title="Resource catalog"
                        value="148"
                        caption="Gravity variables are still mapped through Octo tokens"
                        icon={<Database width={18} height={18} />}
                    />
                </div>

                <SectionPanel
                    title="FHIR components"
                    description="Shared primitives inherit the same override without local styling."
                    style={{ boxShadow: "0 0 0 4px var(--demo-theme-ring)" }}
                >
                    <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginBottom: 12 }}>
                        <StatusBadge tone="success">Ready</StatusBadge>
                        <ResourceTypeBadge resourceType="Patient" category="fhir" />
                        <CapabilityFlag label="SQL" enabled />
                        <CapabilityFlag label="CQL" />
                    </div>
                    <CommandCard
                        title="Resource browser"
                        description="A widget composed from primitives still follows the provider theme."
                        status="Ready"
                        statusTone="success"
                        meta={[
                            { id: "fhir", label: "FHIR", tone: "info" },
                            { id: "search", label: "search", tone: "neutral" },
                        ]}
                    />
                </SectionPanel>

                <div>
                    <Button view="action">Action button</Button>
                </div>
            </div>
        </UIProvider>
    ),
};
