import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "../Button";
import { Popover } from "./index";

const meta: Meta<typeof Popover> = {
	title: "Overlays/Popover",
	component: Popover,
	tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Popover>;

export const Default: Story = {
	render: () => (
		<Popover
			placement="bottom-start"
			content={<div style={{ padding: 12, maxWidth: 240 }}>Popover content with details.</div>}
		>
			<Button>Open popover</Button>
		</Popover>
	),
};
