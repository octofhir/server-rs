import { type JSX, splitProps, Show } from "solid-js";
import styles from "./Input.module.css";

export interface InputProps extends JSX.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
  fullWidth?: boolean;
  icon?: JSX.Element;
}

export const Input = (props: InputProps) => {
  const [local, rest] = splitProps(props, [
    "label",
    "error",
    "fullWidth",
    "class",
    "id",
    "icon",
  ]);

  const inputId = local.id || `input-${Math.random().toString(36).slice(2)}`;

  return (
    <div
      class={`${styles.container} ${local.fullWidth ? styles.fullWidth : ""} ${local.class || ""}`}
    >
      <Show when={local.label}>
        <label class={styles.label} for={inputId}>
          {local.label}
        </label>
      </Show>
      <div class={`${styles.inputWrapper} ${local.icon ? styles.hasIcon : ""}`}>
        <Show when={local.icon}>
          <span class={styles.icon}>{local.icon}</span>
        </Show>
        <input
          id={inputId}
          class={`${styles.input} ${local.error ? styles.error : ""}`}
          {...rest}
        />
      </div>
      <Show when={local.error}>
        <span class={styles.errorText}>{local.error}</span>
      </Show>
    </div>
  );
};
