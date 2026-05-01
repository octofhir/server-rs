import type { Meta, StoryObj } from "@storybook/react-vite";
import { Boxes3, Folder, House, Terminal } from "@gravity-ui/icons";
import { DashboardShell } from "../widgets";
import { CommandCard, MetricTile, PageHeader, SectionPanel } from "../shared/ui";

const meta: Meta<typeof DashboardShell> = {
    title: "Widgets/Dashboard Shell",
    component: DashboardShell,
    tags: ["autodocs"],
    parameters: {
        layout: "fullscreen",
    },
};

export default meta;
type Story = StoryObj<typeof DashboardShell>;

const shellMenuGroups = [
    {
        id: "main",
        title: "Main",
        items: [
            { id: "dashboard", title: "Dashboard", icon: House, active: true },
            { id: "resources", title: "Resources", icon: Folder },
            { id: "console", title: "REST Console", icon: Terminal },
            { id: "packages", title: "Packages", icon: Boxes3 },
        ],
    },
    {
        id: "operations",
        title: "Operations",
        items: [
            { id: "requests", title: "Requests", icon: Terminal },
            { id: "definitions", title: "Definitions", icon: Folder },
        ],
    },
];

function RegressionContent({ rows = 18 }: { rows?: number }) {
    return (
        <div style={{ display: "grid", gap: 20 }}>
            <PageHeader
                eyebrow="Application shell"
                title="Sidebar spacing and scroll"
                description="Long clinical workspace content stays inside the layout while navigation remains fixed."
            />

            <div style={{ display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12 }}>
                <MetricTile title="Resources" value="148" caption="FHIR definitions" />
                <MetricTile title="Operations" value="32" caption="Available commands" />
                <MetricTile title="Latency" value="42 ms" caption="P95 response" />
            </div>

            <SectionPanel title="Scrollable content">
                <div style={{ display: "grid", gap: 12 }}>
                    {Array.from({ length: rows }, (_, index) => (
                        <CommandCard
                            key={index}
                            title={`FHIR workspace row ${index + 1}`}
                            description="A repeated row used to verify that page scrolling remains inside AppLayout content."
                            status={index % 3 === 0 ? "Ready" : "Queued"}
                            statusTone={index % 3 === 0 ? "success" : "info"}
                        />
                    ))}
                </div>
            </SectionPanel>
        </div>
    );
}

export const GroupedNavigationWithScroll: Story = {
    render: () => (
        <DashboardShell
            logo={{ text: "OctoFHIR" }}
            menuGroups={shellMenuGroups}
            collapseBelow={0}
            status={{ label: "HEALTHY", theme: "success" }}
        >
            <RegressionContent />
        </DashboardShell>
    ),
};

export const CollapsedNavigationRegression: Story = {
    render: () => (
        <DashboardShell
            logo={{ text: "OctoFHIR" }}
            menuGroups={shellMenuGroups}
            defaultPinned={false}
            collapseBelow={0}
            status={{ label: "HEALTHY", theme: "success" }}
        >
            <RegressionContent rows={10} />
        </DashboardShell>
    ),
};
