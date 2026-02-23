import { ActionIcon as MantineActionIcon, type ActionIconProps } from "@mantine/core";
import classes from "./ActionIcon.module.css";

export const ActionIcon = MantineActionIcon.extend({
    classNames: classes,
});

export type { ActionIconProps };
