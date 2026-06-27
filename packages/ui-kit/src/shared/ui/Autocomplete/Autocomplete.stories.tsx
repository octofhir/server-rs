import { useMemo, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Autocomplete, type AutocompleteData } from "./Autocomplete";

const COMMANDS = [
    "Create Patient",
    "Create Observation",
    "Open SQL console",
    "Open GraphQL playground",
    "Run ViewDefinition",
    "Search Encounters",
    "Import package",
    "View audit log",
];

const meta: Meta<typeof Autocomplete> = {
    title: "Form controls/Autocomplete",
    component: Autocomplete,
    tags: ["autodocs"],
    parameters: { layout: "padded" },
};
export default meta;
type Story = StoryObj<typeof Autocomplete>;

export const Basic: Story = {
    render: () => {
        const [value, setValue] = useState("");
        return (
            <div style={{ maxWidth: 360 }}>
                <Autocomplete
                    label="Command"
                    placeholder="Type a command…"
                    data={COMMANDS}
                    value={value}
                    onChange={setValue}
                    clearable
                />
                <p style={{ fontSize: 13, marginTop: 8 }}>Input: {value || "—"}</p>
            </div>
        );
    },
};

export const ServerSuggestions: Story = {
    render: () => {
        const [value, setValue] = useState("");
        const data = useMemo<AutocompleteData>(() => {
            const q = value.trim().toLowerCase();
            if (!q) return [];
            return COMMANDS.filter((c) => c.toLowerCase().includes(q));
        }, [value]);
        return (
            <div style={{ maxWidth: 360 }}>
                <Autocomplete
                    label="Search (server)"
                    placeholder="Type to fetch…"
                    filter="server"
                    data={data}
                    value={value}
                    onChange={setValue}
                    onPick={(v) => console.log("picked", v)}
                />
            </div>
        );
    },
};
