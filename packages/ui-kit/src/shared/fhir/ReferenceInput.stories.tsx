import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { type FhirReference, ReferenceInput, type ReferenceCandidate } from "./ReferenceInput";

const PATIENTS: ReferenceCandidate[] = [
    { reference: "Patient/1", display: "Alice Smith", resourceType: "Patient", secondary: "DOB 1985-04-12" },
    { reference: "Patient/2", display: "Bob Jones", resourceType: "Patient", secondary: "DOB 1971-11-03" },
    { reference: "Patient/3", display: "Carmen Diaz", resourceType: "Patient", secondary: "DOB 1990-02-20" },
    { reference: "Patient/4", display: "David Okafor", resourceType: "Patient", secondary: "DOB 2001-07-09" },
    { reference: "Patient/5", display: "Eve Larsson", resourceType: "Patient", secondary: "DOB 1963-12-30" },
];

// Simulated server search with latency.
function fakeSearch({ query }: { query: string }): Promise<ReferenceCandidate[]> {
    const q = query.toLowerCase();
    return new Promise((resolve) => {
        setTimeout(() => {
            resolve(PATIENTS.filter((p) => p.display?.toLowerCase().includes(q)));
        }, 300);
    });
}

const meta: Meta<typeof ReferenceInput> = {
    title: "Healthcare/FHIR Inputs/ReferenceInput",
    component: ReferenceInput,
    tags: ["autodocs"],
    parameters: { layout: "padded" },
};
export default meta;
type Story = StoryObj<typeof ReferenceInput>;

export const Basic: Story = {
    render: () => {
        const [value, setValue] = useState<FhirReference | null>(null);
        return (
            <div style={{ maxWidth: 380 }}>
                <ReferenceInput
                    label="Subject"
                    resourceType="Patient"
                    search={fakeSearch}
                    value={value}
                    onChange={setValue}
                />
                <pre style={{ fontSize: 12, marginTop: 12 }}>{JSON.stringify(value, null, 2)}</pre>
            </div>
        );
    },
};

export const Prefilled: Story = {
    render: () => {
        const [value, setValue] = useState<FhirReference | null>({
            reference: "Patient/2",
            display: "Bob Jones",
            type: "Patient",
        });
        return (
            <div style={{ maxWidth: 380 }}>
                <ReferenceInput
                    label="Subject"
                    resourceType="Patient"
                    search={fakeSearch}
                    value={value}
                    onChange={setValue}
                />
            </div>
        );
    },
};
