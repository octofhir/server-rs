import type { Meta, StoryObj } from "@storybook/react";
import { NavLink } from "@mantine/core";
import { Stack } from "@mantine/core";
import {
  IconHome,
  IconDatabase,
  IconSearch,
  IconSettings,
  IconShield,
  IconChartBar,
} from "@tabler/icons-react";

const meta: Meta<typeof NavLink> = {
  title: "Navigation/NavLink",
  component: NavLink,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "subtle"],
    },
    active: { control: "boolean" },
    disabled: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof NavLink>;

export const Default: Story = {
  args: {
    label: "Dashboard",
    leftSection: <IconHome size={18} />,
  },
  decorators: [(Story) => <div style={{ width: 240 }}><Story /></div>],
};

export const Active: Story = {
  args: {
    label: "Dashboard",
    leftSection: <IconHome size={18} />,
    active: true,
  },
  decorators: [(Story) => <div style={{ width: 240 }}><Story /></div>],
};

export const Sidebar: Story = {
  render: () => (
    <Stack gap={0} style={{ width: 240 }}>
      <NavLink label="Dashboard" leftSection={<IconHome size={18} />} active />
      <NavLink label="Resources" leftSection={<IconDatabase size={18} />} />
      <NavLink label="Search" leftSection={<IconSearch size={18} />} />
      <NavLink label="Analytics" leftSection={<IconChartBar size={18} />} />
      <NavLink label="Auth" leftSection={<IconShield size={18} />} />
      <NavLink label="Settings" leftSection={<IconSettings size={18} />} />
    </Stack>
  ),
};

export const WithDescription: Story = {
  render: () => (
    <Stack gap={0} style={{ width: 280 }}>
      <NavLink
        label="Patient"
        description="1,234 resources"
        leftSection={<IconDatabase size={18} />}
        active
      />
      <NavLink
        label="Observation"
        description="56,789 resources"
        leftSection={<IconDatabase size={18} />}
      />
      <NavLink
        label="Encounter"
        description="12,345 resources"
        leftSection={<IconDatabase size={18} />}
      />
    </Stack>
  ),
};

export const Nested: Story = {
  render: () => (
    <Stack gap={0} style={{ width: 260 }}>
      <NavLink label="Resources" leftSection={<IconDatabase size={18} />} defaultOpened>
        <NavLink label="Patient" />
        <NavLink label="Observation" />
        <NavLink label="Encounter" />
      </NavLink>
      <NavLink label="Settings" leftSection={<IconSettings size={18} />}>
        <NavLink label="General" />
        <NavLink label="Security" />
      </NavLink>
    </Stack>
  ),
};
