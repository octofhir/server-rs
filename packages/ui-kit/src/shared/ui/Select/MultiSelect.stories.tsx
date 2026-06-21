import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { MultiSelect } from "./index";

const meta: Meta<typeof MultiSelect> = {
	title: "Form Controls/MultiSelect",
	component: MultiSelect,
	tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof MultiSelect>;

const grantTypes = [
	{ value: "authorization_code", label: "Authorization Code" },
	{ value: "client_credentials", label: "Client Credentials" },
	{ value: "refresh_token", label: "Refresh Token" },
	{ value: "password", label: "Password" },
	{ value: "implicit", label: "Implicit" },
];

export const Default: Story = {
	render: () => {
		const [value, setValue] = useState<string[]>(["authorization_code", "refresh_token"]);
		return (
			<div style={{ width: 360 }}>
				<MultiSelect
					label="Grant Types"
					required
					placeholder="Select grant types..."
					data={grantTypes}
					value={value}
					onChange={setValue}
				/>
			</div>
		);
	},
};
