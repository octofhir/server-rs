import { Badge as MantineBadge, type BadgeProps } from "@mantine/core";

export function Badge(props: BadgeProps) {
    return <MantineBadge {...props} />;
}

export type { BadgeProps };
