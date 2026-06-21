import type { Meta, StoryObj } from "@storybook/react-vite";
import { TextInput } from "./index";

const meta: Meta<typeof TextInput> = {
    title: "Form Controls/TextInput",
    component: TextInput,
    tags: ["autodocs"],
    argTypes: {
        size: { control: "select", options: ["s", "m", "l"] },
        disabled: { control: "boolean" },
        error: { control: "boolean" },
    },
};

export default meta;
type Story = StoryObj<typeof TextInput>;

export const Default: Story = {
    args: { placeholder: "Enter text..." },
};
