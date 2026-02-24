import { Badge as MantineBadge, type BadgeProps } from "@octofhir/ui-kit";
import classes from "./Badge.module.css";

export function Badge(props: BadgeProps) {
    return <MantineBadge {...props} classNames={classes} />;
}
