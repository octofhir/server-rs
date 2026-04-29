import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeIcon } from "./ThemeIcon";
import {
  IconHeart,
  IconUser,
  IconStethoscope,
  IconPill,
  IconActivity,
} from "@gravity-ui/icons";

const meta: Meta<typeof ThemeIcon> = {
  title: "Data Display/ThemeIcon",
  component: ThemeIcon,
  tags: ["autodocs"],
  argTypes: {
    view: {
      control: "select",
      options: ["normal", "light", "outlined"],
    },
    size: {
      control: "select",
      options: ["xs", "s", "m", "l", "xl"],
    },
    color: {
      control: "select",
      options: ["primary", "positive", "warning", "danger", "neutral"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof ThemeIcon>;

export const Default: Story = {
  args: {
    children: <IconHeart size={18} />,
    view: "light",
    color: "primary",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <ThemeIcon view="normal"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon view="light"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon view="outlined"><IconHeart size={18} /></ThemeIcon>
    </div>
  ),
};

export const Colors: Story = {
  render: () => (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
      <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
        <span style={{ width: 60, fontSize: 14 }}>Normal</span>
        <ThemeIcon view="normal" color="primary"><IconUser size={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="positive"><IconActivity size={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="warning"><IconPill size={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="danger"><IconStethoscope size={18} /></ThemeIcon>
        <ThemeIcon view="normal" color="neutral"><IconHeart size={18} /></ThemeIcon>
      </div>
      <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
        <span style={{ width: 60, fontSize: 14 }}>Light</span>
        <ThemeIcon view="light" color="primary"><IconUser size={18} /></ThemeIcon>
        <ThemeIcon view="light" color="positive"><IconActivity size={18} /></ThemeIcon>
        <ThemeIcon view="light" color="warning"><IconPill size={18} /></ThemeIcon>
        <ThemeIcon view="light" color="danger"><IconStethoscope size={18} /></ThemeIcon>
        <ThemeIcon view="light" color="neutral"><IconHeart size={18} /></ThemeIcon>
      </div>
      <div style={{ display: "flex", gap: "8px", alignItems: "center" }}>
        <span style={{ width: 60, fontSize: 14 }}>Outlined</span>
        <ThemeIcon view="outlined" color="primary"><IconUser size={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="positive"><IconActivity size={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="warning"><IconPill size={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="danger"><IconStethoscope size={18} /></ThemeIcon>
        <ThemeIcon view="outlined" color="neutral"><IconHeart size={18} /></ThemeIcon>
      </div>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <ThemeIcon size="xs"><IconHeart size={12} /></ThemeIcon>
      <ThemeIcon size="s"><IconHeart size={14} /></ThemeIcon>
      <ThemeIcon size="m"><IconHeart size={18} /></ThemeIcon>
      <ThemeIcon size="l"><IconHeart size={22} /></ThemeIcon>
      <ThemeIcon size="xl"><IconHeart size={28} /></ThemeIcon>
    </div>
  ),
};
