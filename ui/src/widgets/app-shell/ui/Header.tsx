import { useTheme } from "@/app/providers";
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

  const getThemeIcon = () => {
    const current = theme();
    if (current === "system") return "Auto";
    if (current === "light") return "Light";
    return "Dark";
  };

  return (
    <header class={styles.header}>
      <div class={styles.logo}>
        <span class={styles.logoText}>OctoFHIR</span>
        <span class={styles.badge}>Server UI</span>
      </div>

      <div class={styles.actions}>
        <span class={styles.themeLabel}>{getThemeIcon()}</span>
        <button
          type="button"
          class={styles.themeToggle}
          onClick={toggleTheme}
          aria-label="Toggle theme"
        >
          {effectiveTheme() === "dark" ? "🌙" : "☀️"}
        </button>
      </div>
    </header>
  );
};
