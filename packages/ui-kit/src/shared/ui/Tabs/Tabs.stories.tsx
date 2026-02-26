import type { Meta, StoryObj } from "@storybook/react";
import { Tabs } from "@mantine/core";
import { Text } from "@mantine/core";
import {
  IconCode,
  IconTable,
  IconBraces,
  IconSettings,
  IconDatabase,
} from "@tabler/icons-react";

const meta: Meta<typeof Tabs> = {
  title: "Data Display/Tabs",
  component: Tabs,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["default", "outline", "pills"],
    },
    orientation: {
      control: "select",
      options: ["horizontal", "vertical"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Tabs>;

export const Default: Story = {
  render: () => (
    <Tabs defaultValue="json">
      <Tabs.List>
        <Tabs.Tab value="json">JSON</Tabs.Tab>
        <Tabs.Tab value="xml">XML</Tabs.Tab>
        <Tabs.Tab value="table">Table</Tabs.Tab>
      </Tabs.List>
      <Tabs.Panel value="json" pt="sm">
        <Text size="sm">JSON representation of the resource</Text>
      </Tabs.Panel>
      <Tabs.Panel value="xml" pt="sm">
        <Text size="sm">XML representation of the resource</Text>
      </Tabs.Panel>
      <Tabs.Panel value="table" pt="sm">
        <Text size="sm">Tabular view of the resource</Text>
      </Tabs.Panel>
    </Tabs>
  ),
  decorators: [(Story) => <div style={{ width: 500 }}><Story /></div>],
};

export const WithIcons: Story = {
  render: () => (
    <Tabs defaultValue="json">
      <Tabs.List>
        <Tabs.Tab value="json" leftSection={<IconBraces size={16} />}>
          JSON
        </Tabs.Tab>
        <Tabs.Tab value="table" leftSection={<IconTable size={16} />}>
          Table
        </Tabs.Tab>
        <Tabs.Tab value="query" leftSection={<IconCode size={16} />}>
          Query
        </Tabs.Tab>
      </Tabs.List>
      <Tabs.Panel value="json" pt="sm">
        <Text size="sm">JSON view</Text>
      </Tabs.Panel>
      <Tabs.Panel value="table" pt="sm">
        <Text size="sm">Table view</Text>
      </Tabs.Panel>
      <Tabs.Panel value="query" pt="sm">
        <Text size="sm">Query editor</Text>
      </Tabs.Panel>
    </Tabs>
  ),
  decorators: [(Story) => <div style={{ width: 500 }}><Story /></div>],
};

export const Pills: Story = {
  render: () => (
    <Tabs variant="pills" defaultValue="db">
      <Tabs.List>
        <Tabs.Tab value="db" leftSection={<IconDatabase size={16} />}>
          Database
        </Tabs.Tab>
        <Tabs.Tab value="settings" leftSection={<IconSettings size={16} />}>
          Settings
        </Tabs.Tab>
      </Tabs.List>
      <Tabs.Panel value="db" pt="sm">
        <Text size="sm">Database console</Text>
      </Tabs.Panel>
      <Tabs.Panel value="settings" pt="sm">
        <Text size="sm">Server settings</Text>
      </Tabs.Panel>
    </Tabs>
  ),
  decorators: [(Story) => <div style={{ width: 400 }}><Story /></div>],
};
