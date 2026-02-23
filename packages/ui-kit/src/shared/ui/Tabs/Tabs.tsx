import { Tabs as MantineTabs, type TabsProps } from "@mantine/core";
import classes from "./Tabs.module.css";

export const Tabs = MantineTabs.extend({
    classNames: classes,
});

export type { TabsProps };
