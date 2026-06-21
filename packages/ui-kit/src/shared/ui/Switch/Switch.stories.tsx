import type { Meta, StoryObj } from "@storybook/react-vite";
import { Switch } from "./index";

const meta: Meta<typeof Switch> = {
    title: "Form Controls/Switch",
    component: Switch,
    tags: ["autodocs"],
    argTypes: {
        size: { control: "select", options: ["s", "m"] },
        disabled: { control: "boolean" },
        checked: { control: "boolean" },
    },
};

export default meta;
type Story = StoryObj<typeof Switch>;

export const Default: Story = {
    args: { label: "Switch label" },
};

export const WithDescription: Story = {
    args: { label: "Enable feature", description: "Turns the thing on and off." },
};
