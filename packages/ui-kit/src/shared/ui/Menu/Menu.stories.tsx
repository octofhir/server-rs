import type { Meta, StoryObj } from "@storybook/react-vite";
import { Gear, Pencil, TrashBin } from "@gravity-ui/icons";
import { Menu } from "./index";
import { Button } from "../Button";

const meta: Meta<typeof Menu> = {
    title: "Overlays/Menu",
    component: Menu,
    tags: ["autodocs"],
};

export default meta;
type Story = StoryObj<typeof Menu>;

export const Default: Story = {
    render: () => (
        <Menu position="bottom-start">
            <Menu.Target>
                <Button>Open Menu</Button>
            </Menu.Target>
            <Menu.Dropdown>
                <Menu.Item leftSection={<Pencil width={14} height={14} />} onClick={() => alert("Edit")}>
                    Edit
                </Menu.Item>
                <Menu.Item leftSection={<Gear width={14} height={14} />} onClick={() => alert("Settings")}>
                    Settings
                </Menu.Item>
                <Menu.Divider />
                <Menu.Item
                    color="danger"
                    leftSection={<TrashBin width={14} height={14} />}
                    onClick={() => alert("Delete")}
                >
                    Delete
                </Menu.Item>
            </Menu.Dropdown>
        </Menu>
    ),
};
