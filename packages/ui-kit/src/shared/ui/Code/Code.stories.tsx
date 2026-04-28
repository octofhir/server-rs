import type { Meta, StoryObj } from "@storybook/react-vite";
import { Code } from "./index";

const meta: Meta<typeof Code> = {
    title: "Data Display/Code",
    component: Code,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Code>;

export const Default: Story = {
    render: () => <Code>console.log('Hello, world!');</Code>,
};

export const Inline: Story = {
    render: () => (
        <p>
            Use the <Code>useEffect</Code> hook to run side effects after render.
        </p>
    ),
};
