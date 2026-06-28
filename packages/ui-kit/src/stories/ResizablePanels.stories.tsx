import type { Meta, StoryObj } from "@storybook/react-vite";
import { Resizable } from "../shared/ui/ResizablePanels/ResizablePanels";
import { Text } from "../shared/ui/Text";

const meta: Meta = {
    title: "Layout/ResizablePanels",
    component: Resizable.Group,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj;

export const Horizontal: Story = {
    render: () => (
        <div style={{ height: 300, width: "100%", border: "1px solid var(--octo-border-strong)", borderRadius: 8, overflow: "hidden" }}>
            <Resizable.Group orientation="horizontal">
                <Resizable.Pane defaultSize={30} minSize={20}>
                    <div style={{ padding: 16, height: "100%", background: "var(--octo-surface-1)" }}>
                        <Text variant="header-2">Left Pane</Text>
                        <Text color="secondary">Minimum size: 20%</Text>
                    </div>
                </Resizable.Pane>
                <Resizable.Handle />
                <Resizable.Pane defaultSize={70} minSize={30}>
                    <div style={{ padding: 16, height: "100%", background: "var(--octo-surface-2)" }}>
                        <Text variant="header-2">Right Pane</Text>
                        <Text color="secondary">Drag the separator to resize. The handle highlights smoothly.</Text>
                    </div>
                </Resizable.Pane>
            </Resizable.Group>
        </div>
    ),
};

export const Vertical: Story = {
    render: () => (
        <div style={{ height: 400, width: "100%", border: "1px solid var(--octo-border-strong)", borderRadius: 8, overflow: "hidden" }}>
            <Resizable.Group orientation="vertical">
                <Resizable.Pane defaultSize={40} minSize={20}>
                    <div style={{ padding: 16, height: "100%", background: "var(--octo-surface-1)" }}>
                        <Text variant="header-2">Top Pane</Text>
                    </div>
                </Resizable.Pane>
                <Resizable.Handle />
                <Resizable.Pane defaultSize={60} minSize={20}>
                    <div style={{ padding: 16, height: "100%", background: "var(--octo-surface-2)" }}>
                        <Text variant="header-2">Bottom Pane</Text>
                    </div>
                </Resizable.Pane>
            </Resizable.Group>
        </div>
    ),
};
