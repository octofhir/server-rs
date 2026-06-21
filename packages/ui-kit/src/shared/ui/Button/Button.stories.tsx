import type { Meta, StoryObj } from "@storybook/react-vite";
import { Button } from "./Button";

const meta: Meta<typeof Button> = {
  title: "Form Controls/Button",
  component: Button,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "outline", "subtle", "default", "transparent"],
    },
    color: {
      control: "select",
      options: ["primary", "red", "green", "orange", "gray"],
    },
    size: {
      control: "select",
      options: ["xs", "sm", "md", "lg", "xl"],
    },
    pin: {
      control: "select",
      options: [
        "round-round",
        "brick-brick",
        "clear-clear",
        "round-brick",
        "brick-round",
        "round-clear",
        "clear-round",
        "brick-clear",
        "clear-brick",
      ],
    },
    disabled: { control: "boolean" },
    loading: { control: "boolean" },
    selected: { control: "boolean" },
    width: {
      control: "select",
      options: ["auto", "max"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Button>;

export const Default: Story = {
  args: {
    children: "Button",
    variant: "default",
  },
};

export const Views: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px", flexWrap: "wrap" }}>
      <Button variant="default">Normal</Button>
      <Button variant="filled">Action</Button>
      <Button variant="outline">Outlined</Button>
      <Button variant="outline" color="primary">Outlined Info</Button>
      <Button variant="outline" color="red">Outlined Danger</Button>
      <Button variant="filled">Raised</Button>
      <Button variant="subtle">Flat</Button>
      <Button variant="subtle" color="primary">Flat Info</Button>
      <Button variant="subtle" color="red">Flat Danger</Button>
      <Button variant="subtle">Flat Secondary</Button>
    </div>
  ),
};

export const Sizes: Story = {
  render: () => (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <Button size="xs">Extra Small (xs)</Button>
      <Button size="sm">Small (s)</Button>
      <Button size="md">Medium (m)</Button>
      <Button size="lg">Large (l)</Button>
      <Button size="xl">Extra Large (xl)</Button>
    </div>
  ),
};

export const States: Story = {
  render: () => (
    <div style={{ display: "flex", gap: "8px" }}>
      <Button>Normal</Button>
      <Button disabled>Disabled</Button>
      <Button loading>Loading</Button>
      <Button selected>Selected</Button>
    </div>
  ),
};

export const Pins: Story = {
  render: () => (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
      <div style={{ display: "flex", gap: "1px" }}>
        <Button pin="round-brick">Round Brick</Button>
        <Button pin="brick-brick">Brick Brick</Button>
        <Button pin="brick-round">Brick Round</Button>
      </div>
      <div style={{ display: "flex", gap: "1px" }}>
        <Button pin="round-clear">Round Clear</Button>
        <Button pin="clear-clear">Clear Clear</Button>
        <Button pin="clear-round">Clear Round</Button>
      </div>
    </div>
  ),
};
