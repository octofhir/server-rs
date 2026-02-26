import type { Meta, StoryObj } from "@storybook/react";
import { Alert } from "@mantine/core";
import { Stack } from "@mantine/core";
import {
  IconInfoCircle,
  IconAlertTriangle,
  IconCircleCheck,
  IconAlertCircle,
} from "@tabler/icons-react";

const meta: Meta<typeof Alert> = {
  title: "Feedback/Alert",
  component: Alert,
  tags: ["autodocs"],
  argTypes: {
    variant: {
      control: "select",
      options: ["filled", "light", "outline", "default", "transparent"],
    },
    color: {
      control: "select",
      options: ["primary", "fire", "warm", "deep", "gray"],
    },
  },
};

export default meta;
type Story = StoryObj<typeof Alert>;

export const Default: Story = {
  args: {
    title: "Information",
    children: "FHIR server is running in development mode.",
    icon: <IconInfoCircle />,
  },
  decorators: [(Story) => <div style={{ width: 500 }}><Story /></div>],
};

export const Variants: Story = {
  render: () => (
    <Stack style={{ width: 500 }}>
      <Alert title="Light" variant="light" icon={<IconInfoCircle />}>
        Default light variant with subtle background.
      </Alert>
      <Alert title="Filled" variant="filled" icon={<IconInfoCircle />}>
        Filled variant with solid background.
      </Alert>
      <Alert title="Outline" variant="outline" icon={<IconInfoCircle />}>
        Outline variant with border only.
      </Alert>
    </Stack>
  ),
};

export const StatusAlerts: Story = {
  render: () => (
    <Stack style={{ width: 500 }}>
      <Alert
        color="primary"
        title="Success"
        icon={<IconCircleCheck />}
      >
        Resource successfully created with ID patient-001.
      </Alert>
      <Alert
        color="warm"
        title="Warning"
        icon={<IconAlertTriangle />}
      >
        Search returned partial results due to timeout.
      </Alert>
      <Alert
        color="fire"
        title="Error"
        icon={<IconAlertCircle />}
      >
        OperationOutcome: Resource validation failed.
      </Alert>
      <Alert
        color="primary"
        title="Info"
        icon={<IconInfoCircle />}
      >
        Server supports FHIR R4, R4B, R5, and R6.
      </Alert>
    </Stack>
  ),
};

export const WithClose: Story = {
  args: {
    title: "Deprecation Notice",
    children: "The _format parameter is deprecated. Use Accept header instead.",
    color: "warm",
    icon: <IconAlertTriangle />,
    withCloseButton: true,
  },
  decorators: [(Story) => <div style={{ width: 500 }}><Story /></div>],
};
