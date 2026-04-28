import type { Meta, StoryObj } from "@storybook/react-vite";
import { tokens } from "../shared/theme/tokens";

const meta: Meta = {
    title: "Foundations/Spacing & Radius",
    parameters: { layout: "padded" },
};

export default meta;
type Story = StoryObj;

export const Spacing: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 12, maxWidth: 720 }}>
            <h2 style={{ margin: 0, color: "var(--g-color-text-primary)" }}>Spacing scale</h2>
            <p style={{ margin: 0, color: "var(--g-color-text-secondary)" }}>
                Use these tokens via the <code>w/h/p/m</code> spacing props on layout primitives,
                or read them as design-token values via <code>useDesignTokens()</code>.
            </p>
            {Object.entries(tokens.spacing).map(([key, value]) => (
                <div key={key} style={{ display: "flex", alignItems: "center", gap: 12 }}>
                    <code style={{ width: 60, color: "var(--g-color-text-secondary)" }}>{key}</code>
                    <code style={{ width: 80, color: "var(--g-color-text-secondary)" }}>{value}</code>
                    <div
                        style={{
                            height: 16,
                            width: value,
                            background: "var(--g-color-base-brand)",
                            borderRadius: 4,
                        }}
                    />
                </div>
            ))}
        </div>
    ),
};

export const Radius: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 720 }}>
            <h2 style={{ margin: 0, color: "var(--g-color-text-primary)" }}>Radius scale</h2>
            <div style={{ display: "flex", gap: 16, flexWrap: "wrap" }}>
                {Object.entries(tokens.radius).map(([key, value]) => (
                    <div
                        key={key}
                        style={{
                            display: "flex",
                            flexDirection: "column",
                            alignItems: "center",
                            gap: 6,
                        }}
                    >
                        <div
                            style={{
                                width: 88,
                                height: 88,
                                background: "var(--g-color-base-selection)",
                                border: "1px solid var(--g-color-base-brand)",
                                borderRadius: value,
                            }}
                        />
                        <code style={{ color: "var(--g-color-text-primary)", fontWeight: 600 }}>{key}</code>
                        <code style={{ color: "var(--g-color-text-secondary)", fontSize: 12 }}>{value}</code>
                    </div>
                ))}
            </div>
        </div>
    ),
};

export const Shadow: Story = {
    render: () => (
        <div style={{ display: "flex", gap: 24, padding: 24, flexWrap: "wrap" }}>
            {Object.entries(tokens.shadow).map(([key, value]) => (
                <div
                    key={key}
                    style={{
                        width: 160,
                        height: 100,
                        background: "var(--g-color-base-background)",
                        boxShadow: value,
                        borderRadius: tokens.radius.lg,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        flexDirection: "column",
                        gap: 4,
                        color: "var(--g-color-text-primary)",
                    }}
                >
                    <code style={{ fontWeight: 700 }}>{key}</code>
                    <code style={{ fontSize: 11, color: "var(--g-color-text-secondary)" }}>shadow</code>
                </div>
            ))}
        </div>
    ),
};
