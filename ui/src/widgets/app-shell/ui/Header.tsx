import { Show } from "solid-js";
import { useTheme } from "@/app/providers";
import { IconSun, IconMoon, IconLogOut, IconUser } from "@/shared/ui/Icon";
import { user, logout, isLoading } from "@/entities/auth";
import { HealthBadge } from "./HealthBadge";
import { CommitChip } from "./CommitChip";
import styles from "./Header.module.css";

export const Header = () => {
  const { theme, setTheme, effectiveTheme } = useTheme();

  const toggleTheme = () => {
    const current = theme();
    if (current === "system") {
      setTheme("light");
    } else if (current === "light") {
      setTheme("dark");
    } else {
      setTheme("system");
    }
  };

  const getThemeLabel = () => {
    const current = theme();
    if (current === "system") return "Auto";
    if (current === "light") return "Light";
    return "Dark";
  };

  const handleLogout = async () => {
    await logout();
    // RouteGuard will handle redirect to login
  };

  const displayName = () => {
    const currentUser = user();
    return currentUser?.preferred_username || currentUser?.name || currentUser?.sub || "User";
  };

  return (
    <header class={styles.header}>
      <div class={styles.left}>
        <HealthBadge showRefreshButton={false} />
        <CommitChip />
      </div>

      <div class={styles.right}>
        {/* User info */}
        <Show when={user()}>
          <div class={styles.userInfo}>
            <IconUser size={16} />
            <span class={styles.userName}>{displayName()}</span>
          </div>
        </Show>

        {/* Theme toggle */}
        <span class={styles.themeLabel}>{getThemeLabel()}</span>
        <button
          type="button"
          class={styles.themeToggle}
          onClick={toggleTheme}
          aria-label="Toggle theme"
          title={`Current: ${getThemeLabel()}`}
        >
          {effectiveTheme() === "dark" ? (
            <IconMoon size={18} />
          ) : (
            <IconSun size={18} />
          )}
        </button>

        {/* Logout button */}
        <Show when={user()}>
          <button
            type="button"
            class={styles.logoutButton}
            onClick={handleLogout}
            disabled={isLoading()}
            aria-label="Sign out"
            title="Sign out"
          >
            <IconLogOut size={18} />
          </button>
        </Show>
      </div>
    </header>
  );
};
