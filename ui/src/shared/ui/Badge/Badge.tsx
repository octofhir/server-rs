import { Badge as KitBadge, type BadgeProps } from "@octofhir/ui-kit";
import classes from "./Badge.module.css";

export function Badge({ className, ...props }: BadgeProps) {
    return (
        <KitBadge
            {...props}
            className={[classes.root, className].filter(Boolean).join(" ")}
        />
    );
}
