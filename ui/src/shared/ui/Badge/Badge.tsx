import { Badge as MantineBadge, type BadgeProps } from "@mantine/core";
import classes from "./Badge.module.css";

export function Badge(props: BadgeProps) {
    return <MantineBadge {...props} classNames={classes} />;
}
