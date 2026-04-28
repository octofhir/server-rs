import { forwardRef } from "react";
import { ActionIcon as KitActionIcon, type ActionIconProps } from "@octofhir/ui-kit";
import classes from "./ActionIcon.module.css";

export const ActionIcon = forwardRef<HTMLButtonElement, ActionIconProps>(
    ({ className, ...props }, ref) => {
        return (
            <KitActionIcon
                ref={ref}
                {...(props as ActionIconProps)}
                className={[classes.actionIcon, className].filter(Boolean).join(" ")}
            />
        );
    },
);
ActionIcon.displayName = "ActionIcon";
