import type { Meta, StoryObj } from "@storybook/react-vite";
import { Card } from "./index";

const meta: Meta<typeof Card> = {
	title: "Data Display/Card",
	component: Card,
	tags: ["autodocs"],
	argTypes: {
		variant: {
			control: "select",
			options: ["filled", "outlined", "raised", "clear"],
		},
	},
};

export default meta;
type Story = StoryObj<typeof Card>;

export const Default: Story = {
	args: {
		children: "Card content",
		variant: "outlined",
	},
};

export const Variants: Story = {
	render: () => (
		<div style={{ display: "flex", gap: 16, flexWrap: "wrap", maxWidth: 720 }}>
			{(["filled", "outlined", "raised", "clear"] as const).map((variant) => (
				<Card key={variant} variant={variant} style={{ width: 200 }}>
					<strong style={{ textTransform: "capitalize" }}>{variant}</strong>
					<p style={{ margin: "8px 0 0", color: "var(--octo-text-muted)" }}>
						Surface variant.
					</p>
				</Card>
			))}
		</div>
	),
};
