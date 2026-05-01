import type { Meta, StoryObj } from "@storybook/react-vite";
import { OperationOutcomePanel } from "./OperationOutcomePanel";

const meta: Meta<typeof OperationOutcomePanel> = {
    title: "Healthcare/FHIR Primitives/OperationOutcomePanel",
    component: OperationOutcomePanel,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof OperationOutcomePanel>;

export const ValidationErrors: Story = {
    args: {
        outcome: {
            resourceType: "OperationOutcome",
            issue: [
                {
                    severity: "error",
                    code: "required",
                    diagnostics: "Patient.name is required for this profile.",
                    expression: ["Patient.name"],
                },
                {
                    severity: "warning",
                    code: "business-rule",
                    diagnostics: "Identifier system is not recognized by the configured namespace policy.",
                    expression: ["Patient.identifier[0].system"],
                },
            ],
        },
    },
};

export const Empty: Story = {
    args: {
        outcome: {
            resourceType: "OperationOutcome",
            issue: [],
        },
    },
};

