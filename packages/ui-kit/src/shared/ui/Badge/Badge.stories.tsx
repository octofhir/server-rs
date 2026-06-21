import type { Meta, StoryObj } from "@storybook/react-vite";
import { Badge } from "./Badge";

const meta: Meta<typeof Badge> = {
    title: "Data Display/Badge",
    component: Badge,
    tags: ["autodocs"],
    argTypes: {
        theme: {
            control: "select",
            options: ["normal", "info", "danger", "warning", "success", "unknown", "clear"],
        },
        color: {
            control: "select",
            options: ["primary", "blue", "green", "red", "fire", "warm", "gray"],
        },
        variant: {
            control: "select",
            options: ["light", "filled", "outline", "dot"],
        },
        size: {
            control: "select",
            options: ["xs", "s", "m"],
        },
    },
};

export default meta;
type Story = StoryObj<typeof Badge>;

export const Default: Story = {
    args: {
        children: "Badge",
        color: "primary",
        variant: "light",
    },
};

export const Tones: Story = {
    render: () => (
        <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
            <Badge color="primary">Primary</Badge>
            <Badge color="blue">Info</Badge>
            <Badge color="green">Success</Badge>
            <Badge color="warm">Warning</Badge>
            <Badge color="fire">Danger</Badge>
            <Badge color="gray">Neutral</Badge>
        </div>
    ),
};

export const Variants: Story = {
    render: () => (
        <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
            <Badge variant="light" color="primary">Light</Badge>
            <Badge variant="filled" color="primary">Filled</Badge>
            <Badge variant="outline" color="primary">Outline</Badge>
            <Badge variant="dot" color="green">Dot</Badge>
        </div>
    ),
};

export const Sizes: Story = {
    render: () => (
        <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
            <Badge size="xs">Extra Small</Badge>
            <Badge size="sm">Small</Badge>
            <Badge size="md">Medium</Badge>
        </div>
    ),
};
