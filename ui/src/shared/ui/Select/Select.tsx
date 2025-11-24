import { type JSX, splitProps, Show, For } from "solid-js";
import styles from "./Select.module.css";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps extends Omit<JSX.SelectHTMLAttributes<HTMLSelectElement>, "onChange"> {
  label?: string;
  error?: string;
  options: SelectOption[];
  fullWidth?: boolean;
  onChange?: (value: string) => void;
}

export const Select = (props: SelectProps) => {
  const [local, rest] = splitProps(props, [
    "label",
    "error",
    "options",
    "fullWidth",
    "class",
    "id",
    "onChange",
  ]);

  const selectId = local.id || `select-${Math.random().toString(36).slice(2)}`;

  const handleChange: JSX.EventHandler<HTMLSelectElement, Event> = (e) => {
    local.onChange?.(e.currentTarget.value);
  };

  return (
    <div
      class={`${styles.container} ${local.fullWidth ? styles.fullWidth : ""} ${local.class || ""}`}
    >
      <Show when={local.label}>
        <label class={styles.label} for={selectId}>
          {local.label}
        </label>
      </Show>
      <select
        id={selectId}
        class={`${styles.select} ${local.error ? styles.error : ""}`}
        onChange={handleChange}
        {...rest}
      >
        <For each={local.options}>
          {(option) => <option value={option.value}>{option.label}</option>}
        </For>
      </select>
      <Show when={local.error}>
        <span class={styles.errorText}>{local.error}</span>
      </Show>
    </div>
  );
};
