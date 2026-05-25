import type { Meta, StoryObj } from "@storybook/react-vite";
import * as GravityIcons from "@gravity-ui/icons";
import type { ComponentType, SVGProps } from "react";
import { useState } from "react";
import { TextInput } from "../TextInput";
import { Text } from "../Text";
import { Flex } from "../Flex";
import { Card } from "../Card";
import * as KitIcons from "./index";

interface IconEntry {
    name: string;
    Component: ComponentType<SVGProps<SVGSVGElement>>;
    source: "alias" | "gravity";
}

type IconModule = Record<string, unknown> & {
    default?: Record<string, unknown>;
};

const gravityIconModule = GravityIcons as IconModule;
const gravityIconExports = gravityIconModule.default ?? gravityIconModule;

function isIconComponent(value: unknown): value is IconEntry["Component"] {
    return typeof value === "function";
}

function getIconEntries(
    source: IconEntry["source"],
    icons: Record<string, unknown>,
    namePattern: RegExp,
): IconEntry[] {
    const entries: IconEntry[] = [];

    for (const [name, value] of Object.entries(icons)) {
        if (namePattern.test(name) && isIconComponent(value)) {
            entries.push({ name, Component: value, source });
        }
    }

    return entries;
}

const allIcons: IconEntry[] = [
    ...getIconEntries("alias", KitIcons, /^Icon[A-Z]/),
    ...getIconEntries("gravity", gravityIconExports, /^[A-Z]/),
].sort((a, b) => a.name.localeCompare(b.name));

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
        const normalizedQuery = query.trim().toLowerCase();
        const filtered = normalizedQuery
            ? allIcons.filter(({ name, source }) => {
                  const haystack = `${name} ${source}`.toLowerCase();
                  return haystack.includes(normalizedQuery);
              })
            : allIcons;

        return (
            <Flex direction="column" gap={4}>
                <TextInput
                    placeholder={`Filter ${allIcons.length} icons...`}
                    value={query}
                    onUpdate={setQuery}
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
                    {filtered.map(({ name, Component, source }) => (
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
                                {source === "alias" ? (
                                    <Text variant="caption-2" color="hint">
                                        ui-kit alias
                                    </Text>
                                ) : null}
                            </Flex>
                        </Card>
                    ))}
                </div>
            </Flex>
        );
    },
};
