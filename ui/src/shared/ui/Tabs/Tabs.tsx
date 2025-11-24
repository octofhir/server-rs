import { type ParentComponent, For, createSignal, children as accessChildren } from "solid-js";
import styles from "./Tabs.module.css";

export interface TabItem {
  value: string;
  label: string;
}

export interface TabsProps {
  value: string;
  onChange: (value: string) => void;
  items: TabItem[];
}

export const Tabs: ParentComponent<TabsProps> = (props) => {
  return (
    <div class={styles.container}>
      <div class={styles.tabList} role="tablist">
        <For each={props.items}>
          {(item) => (
            <button
              class={`${styles.tab} ${props.value === item.value ? styles.active : ""}`}
              role="tab"
              aria-selected={props.value === item.value}
              onClick={() => props.onChange(item.value)}
            >
              {item.label}
            </button>
          )}
        </For>
      </div>
      <div class={styles.content}>{props.children}</div>
    </div>
  );
};
