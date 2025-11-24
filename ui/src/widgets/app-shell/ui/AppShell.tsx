import { type ParentComponent } from "solid-js";
import { Header } from "./Header";
import { Sidebar } from "./Sidebar";
import styles from "./AppShell.module.css";

export const AppShell: ParentComponent = (props) => {
  return (
    <div class={styles.container}>
      <Header />
      <div class={styles.body}>
        <Sidebar />
        <main class={styles.main}>{props.children}</main>
      </div>
    </div>
  );
};
