import { Show } from "solid-js";
import styles from "./Loader.module.css";

export interface LoaderProps {
  size?: "sm" | "md" | "lg";
  class?: string;
  label?: string;
  fullScreen?: boolean;
}

export const Loader = (props: LoaderProps) => {
  const spinner = (
    <div class={`${styles.loader} ${styles[props.size || "md"]} ${props.class || ""}`}>
      <div class={styles.spinner} />
      <Show when={props.label}>
        <span class={styles.label}>{props.label}</span>
      </Show>
    </div>
  );

  if (props.fullScreen) {
    return <div class={styles.fullScreen}>{spinner}</div>;
  }

  return spinner;
};
