import { For, type Component } from "solid-js";
import { A, useLocation } from "@solidjs/router";
import {
  IconHome,
  IconFolder,
  IconTerminal,
  IconServer,
  IconSettings,
  IconDatabase,
} from "@/shared/ui/Icon";
import styles from "./Sidebar.module.css";

interface NavItem {
  label: string;
  path: string;
  description: string;
  icon: Component<{ size?: number; class?: string }>;
}

const navigation: NavItem[] = [
  {
    label: "Dashboard",
    path: "/",
    description: "Overview and quick actions",
    icon: IconHome,
  },
  {
    label: "Resource Browser",
    path: "/resources",
    description: "Browse FHIR resources",
    icon: IconFolder,
  },
  {
    label: "REST Console",
    path: "/console",
    description: "Test FHIR API endpoints",
    icon: IconTerminal,
  },
  {
    label: "API Gateway",
    path: "/gateway",
    description: "Manage custom API endpoints",
    icon: IconServer,
  },
  {
    label: "DB Console",
    path: "/db-console",
    description: "SQL Editor & Query Tool",
    icon: IconDatabase,
  },
  {
    label: "Settings",
    path: "/settings",
    description: "Configure server settings",
    icon: IconSettings,
  },
];

export const Sidebar = () => {
  const location = useLocation();

  const isActive = (path: string) => {
    if (path === "/") {
      return location.pathname === "/";
    }
    return location.pathname.startsWith(path);
  };

  return (
    <aside class={styles.sidebar}>
      <div class={styles.brand}>
        <span class={styles.brandIcon}>🐙</span>
        <span class={styles.brandText}>OctoFHIR</span>
      </div>

      <nav class={styles.nav}>
        <div class={styles.navSection}>
          <span class={styles.navSectionTitle}>Main</span>
          <For each={navigation.slice(0, 4)}>
            {(item) => (
              <A
                href={item.path}
                class={styles.navItem}
                classList={{ [styles.active]: isActive(item.path) }}
              >
                <item.icon size={18} class={styles.navIcon} />
                <div class={styles.navContent}>
                  <span class={styles.navLabel}>{item.label}</span>
                  <span class={styles.navDescription}>{item.description}</span>
                </div>
              </A>
            )}
          </For>
        </div>

        <div class={styles.navSection}>
          <span class={styles.navSectionTitle}>Tools</span>
          <For each={navigation.slice(4)}>
            {(item) => (
              <A
                href={item.path}
                class={styles.navItem}
                classList={{ [styles.active]: isActive(item.path) }}
              >
                <item.icon size={18} class={styles.navIcon} />
                <div class={styles.navContent}>
                  <span class={styles.navLabel}>{item.label}</span>
                  <span class={styles.navDescription}>{item.description}</span>
                </div>
              </A>
            )}
          </For>
        </div>
      </nav>

      <div class={styles.footer}>
        <div class={styles.footerText}>FHIR R4 Server</div>
      </div>
    </aside>
  );
};
