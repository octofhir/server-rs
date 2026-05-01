import type { Meta, StoryObj } from "@storybook/react-vite";
import { Database, Shield, Terminal } from "@gravity-ui/icons";
import { RecordList } from "./RecordList";

const meta: Meta<typeof RecordList> = {
    title: "Shared/RecordList",
    component: RecordList,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof RecordList>;

export const ControlPlaneRecords: Story = {
    args: {
        selectedId: "patient",
        items: [
            {
                id: "patient",
                title: "Patient",
                subtitle: "public.patient",
                description: "Administrative patient data used by resource browser and SQL tools.",
                leading: <Database width={18} height={18} />,
                meta: [
                    { id: "category", label: "FHIR", tone: "info" },
                    { id: "rows", label: "12,840 rows", tone: "success" },
                ],
            },
            {
                id: "policy",
                title: "Access policy evaluation",
                subtitle: "auth.policy.evaluate",
                description: "Authorization decision path with matcher and engine context.",
                leading: <Shield width={18} height={18} />,
                meta: [
                    { id: "access", label: "Protected", tone: "success" },
                    { id: "engine", label: "QuickJS", tone: "neutral" },
                ],
            },
            {
                id: "console",
                title: "GET /fhir/Patient",
                subtitle: "86 ms",
                description: "FHIR REST request summary with response metadata from an app adapter.",
                leading: <Terminal width={18} height={18} />,
                meta: [
                    { id: "status", label: "200 OK", tone: "success" },
                    { id: "bundle", label: "Bundle/searchset", tone: "info" },
                ],
            },
        ],
    },
};

export const Empty: Story = {
    args: {
        items: [],
    },
};

export const CompactSchemaRecords: Story = {
    args: {
        density: "compact",
        selectedId: "public.patient",
        items: [
            {
                id: "public.patient",
                title: "patient",
                subtitle: "public",
                description: "~12,840 rows",
                leading: <Database width={16} height={16} />,
                meta: [{ id: "table", label: "table", tone: "neutral" }],
            },
            {
                id: "terminology.value_set",
                title: "value_set",
                subtitle: "terminology",
                description: "~1,482 rows",
                leading: <Database width={16} height={16} />,
                meta: [{ id: "view", label: "view", tone: "info" }],
            },
        ],
    },
};
