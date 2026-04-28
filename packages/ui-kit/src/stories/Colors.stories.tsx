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
                    color: "var(--g-color-text-primary)",
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
                    border: "1px solid var(--g-color-line-generic)",
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
                                color: isLight ? "var(--g-color-text-dark-primary)" : "var(--g-color-text-light-primary)",
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
            <h2 style={{ margin: 0, color: "var(--g-color-text-primary)" }}>Brand palette</h2>
            <p style={{ margin: 0, color: "var(--g-color-text-secondary)", maxWidth: 640 }}>
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
    { label: "Brand", varName: "--g-color-base-brand" },
    { label: "Brand hover", varName: "--g-color-base-brand-hover" },
    { label: "Info", varName: "--g-color-base-info-medium" },
    { label: "Success", varName: "--g-color-base-positive-medium" },
    { label: "Warning", varName: "--g-color-base-warning-medium" },
    { label: "Danger", varName: "--g-color-base-danger-medium" },
    { label: "Misc / accent", varName: "--g-color-base-misc-medium" },
    { label: "Selection", varName: "--g-color-base-selection" },
];

export const SemanticTokens: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 720 }}>
            <h2 style={{ margin: 0, color: "var(--g-color-text-primary)" }}>Semantic tokens</h2>
            <p style={{ margin: 0, color: "var(--g-color-text-secondary)" }}>
                These are the Gravity UI CSS variables the kit maps to the OctoFHIR brand.
                Components from <code>@gravity-ui/uikit</code> read them directly.
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
                        background: "var(--g-color-base-generic)",
                    }}
                >
                    <div style={{ fontWeight: 600, color: "var(--g-color-text-primary)" }}>{t.label}</div>
                    <div
                        style={{
                            width: 60,
                            height: 28,
                            borderRadius: 6,
                            background: `var(${t.varName})`,
                            border: "1px solid var(--g-color-line-generic)",
                        }}
                    />
                    <code style={{ color: "var(--g-color-text-secondary)" }}>{t.varName}</code>
                </div>
            ))}
        </div>
    ),
};
