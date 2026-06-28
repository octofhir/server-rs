import type { Meta, StoryObj } from "@storybook/react-vite";
import { Container } from "./index";

const meta: Meta<typeof Container> = {
    title: "Layout/Container",
    component: Container,
    tags: ["autodocs"],
    argTypes: {
        size: {
            control: "select",
            options: ["xs", "sm", "md", "lg", "xl"],
        },
        fluid: { control: "boolean" },
    },
};

export default meta;
type Story = StoryObj<typeof Container>;

export const Default: Story = {
    args: {
        size: "md",
    },
    render: (args) => (
        <Container {...args}>
            <div
                style={{
                    padding: 16,
                    backgroundColor: "var(--octo-accent-primary-bg)",
                    borderRadius: 8,
                }}
            >
                Container content
            </div>
        </Container>
    ),
};

export const Sizes: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            {(["xs", "sm", "md", "lg", "xl"] as const).map((s) => (
                <Container key={s} size={s}>
                    <div
                        style={{
                            padding: 12,
                            background: "var(--octo-surface-3)",
                            border: "1px dashed var(--octo-border-subtle)",
                            borderRadius: 8,
                        }}
                    >
                        size = "{s}"
                    </div>
                </Container>
            ))}
        </div>
    ),
};
