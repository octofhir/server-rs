import type { Meta, StoryObj } from "@storybook/react-vite";
import { ActionIcon } from "./ActionIcon";
import {
  IconSettings,
  IconTrash,
  IconEdit,
  IconCopy,
  IconRefresh,
  IconPlus,
} from "@gravity-ui/icons";

const meta: Meta<typeof ActionIcon> = {
  title: "Form Controls/ActionIcon",
  component: ActionIcon,
  tags: ["autodocs"],
  argTypes: {
    view: {
      control: "select",
      options: [
        "normal",
        "action",
        "outlined",
        "outlined-info",
        "outlined-danger",
        "raised",
        "flat",
        "flat-info",
        "flat-danger",
        "flat-secondary",
        "normal-contrast",
        "outlined-contrast",
        "flat-contrast",
      ],
    },
    size: {
      control: "select",
      options: ["xs", "s", "m", "l", "xl"],
    },
    disabled: { control: "boolean" },
    loading: { control: "boolean" },
    selected: { control: "boolean" },
  },
};

export default meta;
type Story = StoryObj<typeof ActionIcon>;

export const Default: Story = {
  args: {
    children: <IconSettings size={18} />,
    view: "flat",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
      <ActionIcon view="normal"><IconPlus size={18} /></ActionIcon>
      <ActionIcon view="action"><IconEdit size={18} /></ActionIcon>
      <ActionIcon view="outlined"><IconCopy size={18} /></ActionIcon>
      <ActionIcon view="flat"><IconSettings size={18} /></ActionIcon>
      <ActionIcon view="flat-info"><IconRefresh size={18} /></ActionIcon>
      <ActionIcon view="flat-danger"><IconTrash size={18} /></ActionIcon>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <ActionIcon size="xs"><IconSettings size={14} /></ActionIcon>
      <ActionIcon size="s"><IconSettings size={16} /></ActionIcon>
      <ActionIcon size="m"><IconSettings size={18} /></ActionIcon>
      <ActionIcon size="l"><IconSettings size={20} /></ActionIcon>
      <ActionIcon size="xl"><IconSettings size={24} /></ActionIcon>
    </div>
  ),
};

export const States: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <ActionIcon view="flat"><IconSettings size={18} /></ActionIcon>
      <ActionIcon view="flat" disabled><IconSettings size={18} /></ActionIcon>
      <ActionIcon view="flat" loading><IconSettings size={18} /></ActionIcon>
      <ActionIcon view="flat" selected><IconSettings size={18} /></ActionIcon>
    </div>
  ),
};
