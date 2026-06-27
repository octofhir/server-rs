import { useMemo, useState } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Combobox, type ComboboxData } from "./Combobox";

const FRUITS: ComboboxData = [
    "Apple",
    "Apricot",
    "Banana",
    "Blueberry",
    "Cherry",
    "Grape",
    "Mango",
    "Orange",
    "Peach",
    "Pear",
    "Pineapple",
    "Strawberry",
];

const meta: Meta<typeof Combobox> = {
    title: "Form controls/Combobox",
    component: Combobox,
    tags: ["autodocs"],
    parameters: { layout: "padded" },
};
export default meta;
type Story = StoryObj<typeof Combobox>;

export const Single: Story = {
    render: () => {
        const [value, setValue] = useState<string | null>(null);
        return (
            <div style={{ maxWidth: 320 }}>
                <Combobox
                    label="Favourite fruit"
                    placeholder="Search fruit…"
                    data={FRUITS}
                    value={value}
                    onChange={setValue}
                    clearable
                />
                <p style={{ fontSize: 13, marginTop: 8 }}>Value: {value ?? "—"}</p>
            </div>
        );
    },
};

export const Multiple: Story = {
    render: () => {
        const [value, setValue] = useState<string[]>(["Apple", "Mango"]);
        return (
            <div style={{ maxWidth: 360 }}>
                <Combobox
                    multiple
                    label="Fruit basket"
                    placeholder="Add fruit…"
                    data={FRUITS}
                    value={value}
                    onChange={setValue}
                />
                <p style={{ fontSize: 13, marginTop: 8 }}>Selected: {value.join(", ") || "none"}</p>
            </div>
        );
    },
};

interface Country {
    code: string;
    name: string;
}

const ALL_COUNTRIES: Country[] = [
    { code: "us", name: "United States" },
    { code: "gb", name: "United Kingdom" },
    { code: "de", name: "Germany" },
    { code: "fr", name: "France" },
    { code: "es", name: "Spain" },
    { code: "it", name: "Italy" },
    { code: "nl", name: "Netherlands" },
    { code: "se", name: "Sweden" },
    { code: "no", name: "Norway" },
    { code: "fi", name: "Finland" },
];

export const AsyncServerFilter: Story = {
    render: () => {
        const [value, setValue] = useState<string | null>(null);
        const [query, setQuery] = useState("");
        const [loading, setLoading] = useState(false);

        // Simulated server-side filtering.
        const data = useMemo<ComboboxData>(() => {
            const q = query.trim().toLowerCase();
            const matches = q
                ? ALL_COUNTRIES.filter((c) => c.name.toLowerCase().includes(q))
                : ALL_COUNTRIES;
            return matches.map((c) => ({ value: c.code, label: c.name }));
        }, [query]);

        return (
            <div style={{ maxWidth: 320 }}>
                <Combobox
                    label="Country (server search)"
                    placeholder="Type to search…"
                    filter="server"
                    data={data}
                    loading={loading}
                    value={value}
                    onChange={setValue}
                    onInputChange={(q) => {
                        setQuery(q);
                        setLoading(true);
                        // Simulate latency settling immediately for the story.
                        setLoading(false);
                    }}
                    clearable
                />
                <p style={{ fontSize: 13, marginTop: 8 }}>Value: {value ?? "—"}</p>
            </div>
        );
    },
};

export const Disabled: Story = {
    args: {
        label: "Disabled",
        data: FRUITS,
        disabled: true,
        placeholder: "Unavailable",
    },
};

export const WithError: Story = {
    args: {
        label: "Required field",
        data: FRUITS,
        required: true,
        error: "Please choose a fruit",
        placeholder: "Search…",
    },
};
