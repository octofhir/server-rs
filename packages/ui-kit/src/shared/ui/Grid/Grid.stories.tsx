import type { Meta, StoryObj } from "@storybook/react-vite";
import { Grid, SimpleGrid } from "./Grid";

const meta: Meta<typeof Grid> = {
    title: "Layout/Grid",
    component: Grid,
    tags: ["autodocs"],
    argTypes: {
        gutter: { control: "number" },
    },
};

export default meta;
type Story = StoryObj<typeof Grid>;

const Cell = ({ children }: { children: React.ReactNode }) => (
    <div
        style={{
            padding: 16,
            backgroundColor: "var(--octo-accent-primary-bg)",
            borderRadius: 8,
            textAlign: "center",
        }}
    >
        {children}
    </div>
);

export const TwelveColumn: Story = {
    args: { gutter: 16 },
    render: (args) => (
        <Grid {...args}>
            <Grid.Col span={6}><Cell>span = 6</Cell></Grid.Col>
            <Grid.Col span={6}><Cell>span = 6</Cell></Grid.Col>
            <Grid.Col span={4}><Cell>span = 4</Cell></Grid.Col>
            <Grid.Col span={4}><Cell>span = 4</Cell></Grid.Col>
            <Grid.Col span={4}><Cell>span = 4</Cell></Grid.Col>
            <Grid.Col span={3}><Cell>span = 3</Cell></Grid.Col>
            <Grid.Col span={3}><Cell>span = 3</Cell></Grid.Col>
            <Grid.Col span={3}><Cell>span = 3</Cell></Grid.Col>
            <Grid.Col span={3}><Cell>span = 3</Cell></Grid.Col>
        </Grid>
    ),
};

export const Simple: Story = {
    render: () => (
        <SimpleGrid cols={3} spacing={16}>
            <Cell>Item 1</Cell>
            <Cell>Item 2</Cell>
            <Cell>Item 3</Cell>
            <Cell>Item 4</Cell>
            <Cell>Item 5</Cell>
            <Cell>Item 6</Cell>
        </SimpleGrid>
    ),
};
