import { useTheme } from "@/app/providers";
import { IconSun, IconMoon } from "@/shared/ui/Icon";
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

  return (
    <header class={styles.header}>
      <div class={styles.left}>
        <HealthBadge showRefreshButton={false} />
        <CommitChip />
      </div>

      <div class={styles.right}>
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
      </div>
    </header>
  );
};
