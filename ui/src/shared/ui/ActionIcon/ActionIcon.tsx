import { ActionIcon as MantineActionIcon, type ActionIconProps, createPolymorphicComponent } from "@octofhir/ui-kit";
import { forwardRef } from "react";
import classes from "./ActionIcon.module.css";

const _ActionIcon = forwardRef<HTMLButtonElement, ActionIconProps>((props, ref) => {
    return <MantineActionIcon ref={ref} {...props} classNames={classes} />;
});

export const ActionIcon = createPolymorphicComponent<"button", ActionIconProps>(_ActionIcon);
