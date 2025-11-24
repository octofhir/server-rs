import { For } from "solid-js";
import { A, useLocation } from "@solidjs/router";
import styles from "./Sidebar.module.css";

interface NavItem {
  label: string;
  path: string;
  description: string;
}

const navigation: NavItem[] = [
  {
    label: "Dashboard",
    path: "/",
    description: "Overview and quick actions",
  },
  {
    label: "Resource Browser",
    path: "/resources",
    description: "Browse FHIR resources",
  },
  {
    label: "REST Console",
    path: "/console",
    description: "Test FHIR API endpoints",
  },
  {
    label: "Settings",
    path: "/settings",
    description: "Configure server settings",
  },
  {
    label: "DB Console",
    path: "/db-console",
    description: "SQL Editor & Query Tool",
  },
];

export const Sidebar = () => {
  const location = useLocation();

  return (
    <aside class={styles.sidebar}>
      <div class={styles.title}>Navigation</div>
      <nav class={styles.nav}>
        <For each={navigation}>
          {(item) => (
            <A
              href={item.path}
              class={styles.navItem}
              classList={{ [styles.active]: location.pathname === item.path }}
            >
              <span class={styles.navLabel}>{item.label}</span>
              <span class={styles.navDescription}>{item.description}</span>
            </A>
          )}
        </For>
      </nav>
    </aside>
  );
};
