import type { Meta, StoryObj } from "@storybook/react-vite";
import { Mail, Search } from "lucide-react";
import { Button } from "../shared/ui/Button";
import { NumberInput } from "../shared/ui/NumberInput";
import { PasswordInput } from "../shared/ui/PasswordInput";
import { Select } from "../shared/ui/Select";
import { TextArea } from "../shared/ui/TextArea";
import { TextInput } from "../shared/ui/TextInput";

const meta: Meta = {
	title: "Form Controls/Matrix",
};
export default meta;

type Story = StoryObj;

const SIZES = ["sm", "md", "lg"] as const;

function Row({ label, children }: { label: string; children: React.ReactNode }) {
	return (
		<div style={{ display: "flex", alignItems: "center", gap: 16 }}>
			<div style={{ width: 120, fontSize: 13, color: "var(--octo-text-muted)" }}>{label}</div>
			<div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>{children}</div>
		</div>
	);
}

const options = [
	{ value: "patient", label: "Patient" },
	{ value: "observation", label: "Observation" },
	{ value: "encounter", label: "Encounter" },
];

export const SizesAndStates: Story = {
	render: () => (
		<div style={{ display: "flex", flexDirection: "column", gap: 20, maxWidth: 720 }}>
			{SIZES.map((size) => (
				<div key={size} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
					<strong style={{ fontSize: 12, textTransform: "uppercase", letterSpacing: 0.5 }}>
						size = {size}
					</strong>
					<Row label="Alignment">
						<TextInput size={size} placeholder="Text input" />
						<Button size={size}>Button</Button>
						<Select size={size} placeholder="Select" data={options} />
					</Row>
					<Row label="Default">
						<TextInput size={size} placeholder="Placeholder" />
						<TextInput size={size} defaultValue="Typed value" />
					</Row>
					<Row label="With affix">
						<TextInput size={size} placeholder="Search" leftSection={<Search size={16} />} />
						<TextInput size={size} placeholder="Email" rightSection={<Mail size={16} />} />
					</Row>
					<Row label="Password">
						<PasswordInput size={size} defaultValue="secret123" />
					</Row>
					<Row label="Number">
						<NumberInput size={size} defaultValue={42} />
					</Row>
					<Row label="Error">
						<TextInput size={size} error defaultValue="bad value" />
					</Row>
					<Row label="Disabled">
						<TextInput size={size} disabled defaultValue="disabled" />
					</Row>
				</div>
			))}
			<div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
				<strong style={{ fontSize: 12, textTransform: "uppercase", letterSpacing: 0.5 }}>TextArea</strong>
				<TextArea placeholder="Multi-line text..." minRows={3} />
				<TextArea error defaultValue="invalid" minRows={2} />
			</div>
		</div>
	),
};
