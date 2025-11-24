import { type ParentComponent, splitProps } from "solid-js";
import styles from "./StatusBadge.module.css";

export interface StatusBadgeProps {
  variant?: "success" | "warning" | "error" | "info" | "neutral";
  size?: "sm" | "md";
  class?: string;
}

export const StatusBadge: ParentComponent<StatusBadgeProps> = (props) => {
  const [local, rest] = splitProps(props, ["variant", "size", "class", "children"]);

  const classNames = () => {
    const classes = [styles.badge];
    classes.push(styles[local.variant || "neutral"]);
    classes.push(styles[local.size || "md"]);
    if (local.class) classes.push(local.class);
    return classes.join(" ");
  };

  return (
    <span class={classNames()} {...rest}>
      {local.children}
    </span>
  );
};
