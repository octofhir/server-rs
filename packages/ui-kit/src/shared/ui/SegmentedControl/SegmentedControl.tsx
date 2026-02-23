import { SegmentedControl as MantineSegmentedControl, type SegmentedControlProps } from "@mantine/core";

export function SegmentedControl(props: SegmentedControlProps) {
    return <MantineSegmentedControl {...props} />;
}

export type { SegmentedControlProps };
