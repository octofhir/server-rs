import type { Meta, StoryObj } from "@storybook/react";
import { Menu } from "@mantine/core";
import { Button, Text, Group } from "@mantine/core";
import {
  IconSettings,
  IconTrash,
  IconEdit,
  IconCopy,
  IconDownload,
  IconDots,
} from "@tabler/icons-react";
import { ActionIcon } from "@mantine/core";

const meta: Meta<typeof Menu> = {
  title: "Overlays/Menu",
  component: Menu,
  tags: ["autodocs"],
  argTypes: {
    position: {
      control: "select",
      options: ["bottom", "bottom-start", "bottom-end", "top", "top-start", "top-end"],
    },
    shadow: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Menu>;

export const Default: Story = {
  render: () => (
    <Menu>
      <Menu.Target>
        <Button>Open Menu</Button>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Item leftSection={<IconEdit size={16} />}>Edit</Menu.Item>
        <Menu.Item leftSection={<IconCopy size={16} />}>Duplicate</Menu.Item>
        <Menu.Item leftSection={<IconDownload size={16} />}>Export</Menu.Item>
        <Menu.Divider />
        <Menu.Item color="red" leftSection={<IconTrash size={16} />}>
          Delete
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  ),
};

export const WithSections: Story = {
  render: () => (
    <Menu>
      <Menu.Target>
        <Button variant="default">Actions</Button>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Label>Resource</Menu.Label>
        <Menu.Item leftSection={<IconEdit size={16} />}>Edit resource</Menu.Item>
        <Menu.Item leftSection={<IconCopy size={16} />}>Clone resource</Menu.Item>
        <Menu.Item leftSection={<IconDownload size={16} />}>Export as JSON</Menu.Item>
        <Menu.Divider />
        <Menu.Label>Settings</Menu.Label>
        <Menu.Item leftSection={<IconSettings size={16} />}>Preferences</Menu.Item>
        <Menu.Divider />
        <Menu.Label>Danger zone</Menu.Label>
        <Menu.Item color="red" leftSection={<IconTrash size={16} />}>
          Delete resource
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  ),
};

export const IconTrigger: Story = {
  render: () => (
    <Menu>
      <Menu.Target>
        <ActionIcon variant="subtle">
          <IconDots size={18} />
        </ActionIcon>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Item leftSection={<IconEdit size={16} />}>Edit</Menu.Item>
        <Menu.Item leftSection={<IconCopy size={16} />}>Copy ID</Menu.Item>
        <Menu.Divider />
        <Menu.Item color="red" leftSection={<IconTrash size={16} />}>
          Delete
        </Menu.Item>
      </Menu.Dropdown>
    </Menu>
  ),
};
