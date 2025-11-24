import { type JSX, type ParentComponent, splitProps } from "solid-js";
import styles from "./Button.module.css";

export interface ButtonProps extends JSX.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "outline" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  fullWidth?: boolean;
  loading?: boolean;
}

export const Button: ParentComponent<ButtonProps> = (props) => {
  const [local, rest] = splitProps(props, [
    "variant",
    "size",
    "fullWidth",
    "loading",
    "disabled",
    "children",
    "class",
  ]);

  const classNames = () => {
    const classes = [styles.button];
    classes.push(styles[local.variant || "primary"]);
    classes.push(styles[local.size || "md"]);
    if (local.fullWidth) classes.push(styles.fullWidth);
    if (local.loading) classes.push(styles.loading);
    if (local.class) classes.push(local.class);
    return classes.join(" ");
  };

  return (
    <button
      class={classNames()}
      disabled={local.disabled || local.loading}
      {...rest}
    >
      {local.loading && <span class={styles.spinner} />}
      <span class={local.loading ? styles.hiddenContent : undefined}>
        {local.children}
      </span>
    </button>
  );
};
