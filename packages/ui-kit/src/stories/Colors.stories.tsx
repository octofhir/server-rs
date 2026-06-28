import type { Meta, StoryObj } from "@storybook/react-vite";
import { palette, type PaletteHue } from "../shared/theme/colors";

const meta: Meta = {
    title: "Foundations/Colors",
    parameters: { layout: "padded" },
};

export default meta;
type Story = StoryObj;

const hues: PaletteHue[] = ["primary", "info", "accent", "success", "warning", "error", "deep", "gray"];

function Swatch({ hue }: { hue: PaletteHue }) {
    return (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div
                style={{
                    fontFamily: "var(--g-text-body-font-family)",
                    fontWeight: 600,
                    fontSize: 14,
                    textTransform: "capitalize",
                    color: "var(--octo-text-primary)",
                }}
            >
                {hue}
            </div>
            <div
                style={{
                    display: "grid",
                    gridTemplateColumns: "repeat(10, 1fr)",
                    borderRadius: 8,
                    overflow: "hidden",
                    border: "1px solid var(--octo-border-subtle)",
                }}
            >
                {palette[hue].map((value, idx) => {
                    const stop = idx;
                    const isLight = idx <= 4;
                    return (
                        <div
                            key={idx}
                            title={value}
                            style={{
                                background: value,
                                color: isLight ? "var(--octo-text-primary)" : "var(--octo-text-inverse)",
                                padding: "12px 8px",
                                fontFamily: "var(--g-text-code-font-family)",
                                fontSize: 11,
                                lineHeight: 1.2,
                                textAlign: "center",
                            }}
                        >
                            <div style={{ fontWeight: 700 }}>{stop}</div>
                            {idx === 5 && <div>base</div>}
                        </div>
                    );
                })}
            </div>
        </div>
    );
}

export const Palette: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 24, maxWidth: 980 }}>
            <h2 style={{ margin: 0, color: "var(--octo-text-primary)" }}>Brand palette</h2>
            <p style={{ margin: 0, color: "var(--octo-text-secondary)", maxWidth: 640 }}>
                Each hue runs from 0 (lightest) → 9 (darkest). Stop 5 is the canonical "base"
                used in light mode; stop 4 is preferred in dark mode for stronger legibility.
                All colors are defined in OKLCH for perceptual uniformity.
            </p>
            {hues.map((h) => (
                <Swatch key={h} hue={h} />
            ))}
        </div>
    ),
};

const semanticVars = [
    { label: "Brand", varName: "--octo-accent-primary" },
    { label: "Brand hover", varName: "--octo-accent-primary-hover" },
    { label: "Info", varName: "--octo-brand-info" },
    { label: "Success", varName: "--octo-accent-positive" },
    { label: "Warning", varName: "--octo-accent-warm" },
    { label: "Danger", varName: "--octo-accent-fire" },
    { label: "Misc / accent", varName: "--octo-accent-primary" },
    { label: "Selection", varName: "--octo-accent-primary-bg" },
];

export const SemanticTokens: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 720 }}>
            <h2 style={{ margin: 0, color: "var(--octo-text-primary)" }}>Semantic tokens</h2>
            <p style={{ margin: 0, color: "var(--octo-text-secondary)" }}>
                These are the semantic <code>--octo-*</code> CSS variables that map the
                OctoFHIR brand palette. Components read them directly.
            </p>
            {semanticVars.map((t) => (
                <div
                    key={t.varName}
                    style={{
                        display: "grid",
                        gridTemplateColumns: "200px 60px 1fr",
                        alignItems: "center",
                        gap: 16,
                        padding: 12,
                        borderRadius: 8,
                        background: "var(--octo-surface-3)",
                    }}
                >
                    <div style={{ fontWeight: 600, color: "var(--octo-text-primary)" }}>{t.label}</div>
                    <div
                        style={{
                            width: 60,
                            height: 28,
                            borderRadius: 6,
                            background: `var(${t.varName})`,
                            border: "1px solid var(--octo-border-subtle)",
                        }}
                    />
                    <code style={{ color: "var(--octo-text-secondary)" }}>{t.varName}</code>
                </div>
            ))}
        </div>
    ),
};
