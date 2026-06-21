import type { Meta, StoryObj } from "@storybook/react-vite";
import { Alert } from "./index";

const meta: Meta<typeof Alert> = {
    title: "Feedback/Alert",
    component: Alert,
    tags: ["autodocs"],
    argTypes: {
        theme: {
            control: "select",
            options: ["info", "success", "warning", "danger", "neutral"],
        },
    },
};

export default meta;
type Story = StoryObj<typeof Alert>;

export const Default: Story = {
    args: {
        title: "Alert Title",
        message: "This is an alert message.",
        theme: "info",
    },
};

export const Themes: Story = {
    render: () => (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <Alert theme="info" title="Info" message="Informational message." />
            <Alert theme="success" title="Success" message="Operation completed." />
            <Alert theme="warning" title="Warning" message="Heads up about this." />
            <Alert theme="danger" title="Error" message="Something went wrong." />
        </div>
    ),
};
