import type { Meta, StoryObj } from "@storybook/react-vite";
import * as GravityIcons from "@gravity-ui/icons";
import { useState } from "react";
import { TextInput } from "../TextInput";
import { Text } from "../Text";
import { Flex } from "../Flex";
import { Card } from "../Card";

interface IconEntry {
    name: string;
    Component: React.ComponentType<{ width?: number; height?: number }>;
}

const allIcons: IconEntry[] = Object.entries(GravityIcons)
    .filter(([name, value]) => /^[A-Z]/.test(name) && typeof value === "object" && value !== null)
    .map(([name, Component]) => ({ name, Component: Component as IconEntry["Component"] }))
    .sort((a, b) => a.name.localeCompare(b.name));

const meta: Meta = {
    title: "Foundations/Icons",
    parameters: {
        layout: "padded",
        docs: {
            description: {
                component:
                    "All icons available in the OctoFHIR UI kit. Re-exported from `@gravity-ui/icons`. " +
                    "Import directly from `@octofhir/ui-kit` (Tabler-compat aliases like `IconSearch`) " +
                    "or from `@gravity-ui/icons` (native names like `Magnifier`).",
            },
        },
    },
};

export default meta;

type Story = StoryObj;

export const Catalog: Story = {
    render: () => {
        const [query, setQuery] = useState("");
        const filtered = allIcons.filter((i) =>
            i.name.toLowerCase().includes(query.toLowerCase()),
        );

        return (
            <Flex direction="column" gap={4}>
                <TextInput
                    placeholder={`Filter ${allIcons.length} icons...`}
                    value={query}
                    onUpdate={(v) => setQuery(v)}
                    size="l"
                    hasClear
                />
                <Text variant="caption-2">
                    {filtered.length} of {allIcons.length} icons
                </Text>
                <div
                    style={{
                        display: "grid",
                        gridTemplateColumns: "repeat(auto-fill, minmax(160px, 1fr))",
                        gap: 8,
                    }}
                >
                    {filtered.map(({ name, Component }) => (
                        <Card key={name} type="container" view="filled" theme="normal">
                            <Flex
                                direction="column"
                                alignItems="center"
                                justifyContent="center"
                                gap={2}
                                style={{ padding: 12, minHeight: 96 }}
                            >
                                <Component width={24} height={24} />
                                <Text
                                    variant="caption-2"
                                    color="secondary"
                                    style={{
                                        textAlign: "center",
                                        wordBreak: "break-word",
                                        userSelect: "all",
                                    }}
                                >
                                    {name}
                                </Text>
                            </Flex>
                        </Card>
                    ))}
                </div>
            </Flex>
        );
    },
};
