import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { Chip } from "./Chip";

const meta: Meta<typeof Chip> = {
	title: "Data Display/Chip",
	component: Chip,
	tags: ["autodocs"],
	argTypes: {
		size: {
			control: "select",
			options: ["xs", "s", "m"],
		},
		checked: { control: "boolean" },
		disabled: { control: "boolean" },
	},
};

export default meta;
type Story = StoryObj<typeof Chip>;

export const Default: Story = {
	args: {
		children: "Chip",
		checked: true,
	},
};

export const Toggleable: Story = {
	render: () => {
		const [a, setA] = useState(true);
		const [b, setB] = useState(false);
		const [c, setC] = useState(false);
		return (
			<div style={{ display: "flex", gap: 8 }}>
				<Chip checked={a} onChange={setA}>
					Active
				</Chip>
				<Chip checked={b} onChange={setB}>
					Pending
				</Chip>
				<Chip checked={c} onChange={setC}>
					Archived
				</Chip>
			</div>
		);
	},
};

export const Sizes: Story = {
	render: () => (
		<div style={{ display: "flex", gap: 8, alignItems: "center" }}>
			<Chip size="xs" checked>
				XS
			</Chip>
			<Chip size="sm" checked>
				S
			</Chip>
			<Chip size="md" checked>
				M
			</Chip>
		</div>
	),
};
