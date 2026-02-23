import { Select as MantineSelect, type SelectProps } from "@mantine/core";
import classes from "./Select.module.css";

export const Select = MantineSelect.extend({
    classNames: classes,
});

export type { SelectProps };
