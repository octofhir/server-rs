import type { Meta, StoryObj } from "@storybook/react-vite";
import { useVirtualizer } from "@tanstack/react-virtual";
import * as LucideIcons from "lucide-react";
import type { ComponentType, SVGProps } from "react";
import { useEffect, useRef, useState } from "react";
import { TextInput } from "../TextInput";
import { Text } from "../Text";
import { Flex } from "../Flex";
import { Card } from "../Card";
import * as KitIcons from "./index";

interface IconEntry {
    name: string;
    Component: ComponentType<SVGProps<SVGSVGElement>>;
    source: "alias" | "lucide";
}

type IconModule = Record<string, unknown> & {
    default?: Record<string, unknown>;
};

const lucideIconModule = LucideIcons as IconModule;
const lucideIconExports = lucideIconModule.default ?? lucideIconModule;

// lucide icons are `forwardRef` exotic components (typeof === "object"), not plain
// functions — accept those plus memo/plain-function components.
function isIconComponent(value: unknown): value is IconEntry["Component"] {
    if (typeof value === "function") return true;
    if (typeof value === "object" && value !== null) {
        const tag = (value as { $$typeof?: symbol }).$$typeof;
        return tag === Symbol.for("react.forward_ref") || tag === Symbol.for("react.memo");
    }
    return false;
}

// Names that are not real icons or are duplicate exports we don't want in the catalog.
const NON_ICON_NAMES = new Set(["Icon", "LucideProvider"]);

function isCatalogIcon(name: string, source: IconEntry["source"]): boolean {
    if (NON_ICON_NAMES.has(name)) return false;
    // lucide exports every icon twice (`Activity` and `ActivityIcon`) — keep the bare name.
    if (source === "lucide" && name.endsWith("Icon")) return false;
    return true;
}

function getIconEntries(
    source: IconEntry["source"],
    icons: Record<string, unknown>,
    namePattern: RegExp,
): IconEntry[] {
    const entries: IconEntry[] = [];

    for (const [name, value] of Object.entries(icons)) {
        if (namePattern.test(name) && isCatalogIcon(name, source) && isIconComponent(value)) {
            entries.push({ name, Component: value, source });
        }
    }

    return entries;
}

const allIcons: IconEntry[] = [
    ...getIconEntries("alias", KitIcons, /^Icon[A-Z]/),
    ...getIconEntries("lucide", lucideIconExports, /^[A-Z]/),
].sort((a, b) => a.name.localeCompare(b.name));

const meta: Meta = {
    title: "Foundations/Icons",
    parameters: {
        layout: "padded",
        docs: {
            description: {
                component:
                    "All icons available in the OctoFHIR UI kit, powered by lucide-react. " +
                    "Import the `Icon*` aliases (e.g. `IconSearch`) from `@octofhir/ui-kit`, " +
                    "or any lucide icon directly from `lucide-react`.",
            },
        },
    },
};

export default meta;

type Story = StoryObj;

const COLUMN_MIN = 168; // px per cell
const ROW_HEIGHT = 116; // card (96 min) + gap + label
const GAP = 8;

function IconCatalog() {
    const [query, setQuery] = useState("");
    const scrollRef = useRef<HTMLDivElement>(null);
    const [columns, setColumns] = useState(6);

    // Track container width → responsive column count (no CSS grid auto-fill, since
    // we virtualize by row and need a known column count to slice rows).
    useEffect(() => {
        const el = scrollRef.current;
        if (!el) return;
        const update = () => setColumns(Math.max(1, Math.floor(el.clientWidth / COLUMN_MIN)));
        update();
        const ro = new ResizeObserver(update);
        ro.observe(el);
        return () => ro.disconnect();
    }, []);

    const normalizedQuery = query.trim().toLowerCase();
    const filtered = normalizedQuery
        ? allIcons.filter(({ name, source }) =>
              `${name} ${source}`.toLowerCase().includes(normalizedQuery),
          )
        : allIcons;

    const rowCount = Math.ceil(filtered.length / columns);
    const rowVirtualizer = useVirtualizer({
        count: rowCount,
        getScrollElement: () => scrollRef.current,
        estimateSize: () => ROW_HEIGHT + GAP,
        overscan: 4,
    });

    return (
        <Flex direction="column" gap={4} style={{ height: "100%" }}>
            <TextInput
                placeholder={`Filter ${allIcons.length} icons...`}
                value={query}
                onChange={setQuery}
                size="lg"
            />
            <Text variant="caption-2">
                {filtered.length} of {allIcons.length} icons
            </Text>
            {/* Only the visible rows are mounted — ~4k icons otherwise hang the page. */}
            <div ref={scrollRef} style={{ flex: 1, minHeight: 480, overflowY: "auto" }}>
                <div style={{ position: "relative", height: rowVirtualizer.getTotalSize() }}>
                    {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                        const start = virtualRow.index * columns;
                        const rowItems = filtered.slice(start, start + columns);
                        return (
                            <div
                                key={virtualRow.key}
                                style={{
                                    position: "absolute",
                                    top: 0,
                                    left: 0,
                                    width: "100%",
                                    height: ROW_HEIGHT,
                                    transform: `translateY(${virtualRow.start}px)`,
                                    display: "grid",
                                    gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                                    gap: GAP,
                                }}
                            >
                                {rowItems.map(({ name, Component, source }) => (
                                    <Card key={name} variant="filled" p={0}>
                                        <Flex
                                            direction="column"
                                            align="center"
                                            justify="center"
                                            gap={8}
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
                        );
                    })}
                </div>
            </div>
        </Flex>
    );
}

export const Catalog: Story = {
    render: () => <IconCatalog />,
};
