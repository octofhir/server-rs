import { useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { CodeableConceptInput } from "./CodeableConceptInput";
import { CodingInput } from "./CodingInput";
import type { CodeableConcept as CodeableConceptType, Coding, TerminologyConcept } from "./terminology";

const SNOMED = "http://snomed.info/sct";
const CONCEPTS: TerminologyConcept[] = [
    { system: SNOMED, code: "38341003", display: "Hypertension" },
    { system: SNOMED, code: "73211009", display: "Diabetes mellitus" },
    { system: SNOMED, code: "195967001", display: "Asthma" },
    { system: SNOMED, code: "13645005", display: "Chronic obstructive pulmonary disease" },
    { system: SNOMED, code: "84114007", display: "Heart failure" },
    { system: SNOMED, code: "44054006", display: "Type 2 diabetes mellitus" },
    { system: SNOMED, code: "22298006", display: "Myocardial infarction" },
];

// Simulated ValueSet/$expand with latency + count paging.
function fakeExpand({ query, count = 20 }: { query: string; count?: number }): Promise<TerminologyConcept[]> {
    const q = query.toLowerCase();
    return new Promise((resolve) => {
        setTimeout(() => {
            resolve(CONCEPTS.filter((c) => c.display?.toLowerCase().includes(q)).slice(0, count));
        }, 300);
    });
}

const meta: Meta = {
    title: "Healthcare/FHIR Inputs/Terminology",
    tags: ["autodocs"],
    parameters: { layout: "padded" },
};
export default meta;
type Story = StoryObj;

export const SingleCoding: Story = {
    render: () => {
        const [value, setValue] = useState<Coding | null>(null);
        return (
            <div style={{ maxWidth: 420 }}>
                <CodingInput
                    label="Condition code"
                    valueSet="http://hl7.org/fhir/ValueSet/condition-code"
                    expand={fakeExpand}
                    value={value}
                    onChange={setValue}
                />
                <pre style={{ fontSize: 12, marginTop: 12 }}>{JSON.stringify(value, null, 2)}</pre>
            </div>
        );
    },
};

export const CodeableConcept: Story = {
    render: () => {
        const [value, setValue] = useState<CodeableConceptType | null>(null);
        return (
            <div style={{ maxWidth: 420 }}>
                <CodeableConceptInput
                    label="Conditions"
                    expand={fakeExpand}
                    value={value}
                    onChange={setValue}
                />
                <pre style={{ fontSize: 12, marginTop: 12 }}>{JSON.stringify(value, null, 2)}</pre>
            </div>
        );
    },
};
