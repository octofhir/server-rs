import type { Meta, StoryObj } from "@storybook/react-vite";
import { tokens } from "../shared/theme/tokens";

const meta: Meta = {
    title: "Foundations/Typography",
    parameters: { layout: "padded" },
};

export default meta;
type Story = StoryObj;

const sizes = [
    { key: "xs", label: "Caption / xs", value: tokens.typography.size.xs },
    { key: "sm", label: "Body / sm", value: tokens.typography.size.sm },
    { key: "md", label: "Default / md", value: tokens.typography.size.md },
    { key: "lg", label: "Heading / lg", value: tokens.typography.size.lg },
    { key: "xl", label: "Display / xl", value: tokens.typography.size.xl },
];

const weights = [
    { key: "regular", label: "Regular 400", value: tokens.typography.weight.regular },
    { key: "medium", label: "Medium 500", value: tokens.typography.weight.medium },
    { key: "semibold", label: "Semibold 600", value: tokens.typography.weight.semibold },
    { key: "bold", label: "Bold 700", value: tokens.typography.weight.bold },
];

export const Families: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 24, color: "var(--octo-text-primary)" }}>
            <Sample title="Body — Manrope" font={tokens.typography.family} />
            <Sample title="Heading — Rubik" font={tokens.typography.heading} />
            <Sample title="Code — JetBrains Mono" font={tokens.typography.mono} mono />
        </div>
    ),
};

export const Sizes: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            {sizes.map((s) => (
                <div key={s.key} style={{ display: "flex", alignItems: "baseline", gap: 16 }}>
                    <code style={{ width: 140, color: "var(--octo-text-secondary)" }}>{s.value}</code>
                    <span
                        style={{
                            fontFamily: tokens.typography.family,
                            fontSize: s.value,
                            color: "var(--octo-text-primary)",
                        }}
                    >
                        {s.label} — The quick brown fox jumps over the lazy dog
                    </span>
                </div>
            ))}
        </div>
    ),
};

export const Weights: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            {weights.map((w) => (
                <div
                    key={w.key}
                    style={{
                        fontFamily: tokens.typography.family,
                        fontWeight: w.value,
                        fontSize: 18,
                        color: "var(--octo-text-primary)",
                    }}
                >
                    {w.label} — Healthcare interoperability without compromise.
                </div>
            ))}
        </div>
    ),
};

function Sample({ title, font, mono }: { title: string; font: string; mono?: boolean }) {
    return (
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <div style={{ fontSize: 13, color: "var(--octo-text-secondary)", fontWeight: 600 }}>{title}</div>
            <div style={{ fontFamily: font, fontSize: mono ? 16 : 22, lineHeight: 1.3 }}>
                {mono ? "const Patient = await fhir.read('Patient/123');" : "ABC abc 0123 — Build durable healthcare data infrastructure."}
            </div>
        </div>
    );
}
