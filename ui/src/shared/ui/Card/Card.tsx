import { type ParentComponent, splitProps } from "solid-js";
import styles from "./Card.module.css";

export interface CardProps {
  class?: string;
  padding?: "none" | "sm" | "md" | "lg";
  hoverable?: boolean;
  onClick?: () => void;
}

export const Card: ParentComponent<CardProps> = (props) => {
  const [local, rest] = splitProps(props, [
    "class",
    "padding",
    "hoverable",
    "onClick",
    "children",
  ]);

  const classNames = () => {
    const classes = [styles.card];
    classes.push(styles[`padding-${local.padding || "md"}`]);
    if (local.hoverable) classes.push(styles.hoverable);
    if (local.onClick) classes.push(styles.clickable);
    if (local.class) classes.push(local.class);
    return classes.join(" ");
  };

  return (
    <div class={classNames()} onClick={local.onClick} {...rest}>
      {local.children}
    </div>
  );
};
