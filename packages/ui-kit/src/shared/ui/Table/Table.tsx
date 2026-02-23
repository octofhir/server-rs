import { Table as MantineTable, type TableProps } from "@mantine/core";
import classes from "./Table.module.css";

export const Table = MantineTable.extend({
    classNames: classes,
});

export type { TableProps };
