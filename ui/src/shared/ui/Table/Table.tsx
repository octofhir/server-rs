import { type JSX, type ParentComponent } from "solid-js";
import styles from "./Table.module.css";

export interface TableProps extends JSX.TableHTMLAttributes<HTMLTableElement> {
  striped?: boolean;
  hoverable?: boolean;
  compact?: boolean;
}

export const Table: ParentComponent<TableProps> = (props) => {
  const classNames = () => {
    const classes = [styles.table];
    if (props.striped) classes.push(styles.striped);
    if (props.hoverable) classes.push(styles.hoverable);
    if (props.compact) classes.push(styles.compact);
    if (props.class) classes.push(props.class);
    return classes.join(" ");
  };

  return (
    <div class={styles.tableWrapper}>
      <table class={classNames()}>{props.children}</table>
    </div>
  );
};

export const TableHead: ParentComponent = (props) => {
  return <thead class={styles.thead}>{props.children}</thead>;
};

export const TableBody: ParentComponent = (props) => {
  return <tbody class={styles.tbody}>{props.children}</tbody>;
};

export const TableRow: ParentComponent<JSX.HTMLAttributes<HTMLTableRowElement>> = (props) => {
  return <tr class={props.class}>{props.children}</tr>;
};

export const TableCell: ParentComponent<JSX.TdHTMLAttributes<HTMLTableCellElement>> = (props) => {
  return <td class={props.class}>{props.children}</td>;
};

export const TableHeaderCell: ParentComponent<JSX.ThHTMLAttributes<HTMLTableCellElement>> = (
  props,
) => {
  return <th class={props.class}>{props.children}</th>;
};
