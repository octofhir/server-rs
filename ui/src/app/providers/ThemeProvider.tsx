import {
  createContext,
  useContext,
  createSignal,
  createEffect,
  onMount,
  type ParentComponent,
  type Accessor,
} from "solid-js";

type Theme = "light" | "dark" | "system";

interface ThemeContextValue {
  theme: Accessor<Theme>;
  setTheme: (theme: Theme) => void;
  effectiveTheme: Accessor<"light" | "dark">;
}

const ThemeContext = createContext<ThemeContextValue>();

export const ThemeProvider: ParentComponent = (props) => {
  const [theme, setThemeState] = createSignal<Theme>(
    (localStorage.getItem("octofhir-theme") as Theme) || "system",
  );
  const [effectiveTheme, setEffectiveTheme] = createSignal<"light" | "dark">(
    "light",
  );

  const updateEffectiveTheme = () => {
    const currentTheme = theme();
    if (currentTheme === "system") {
      const prefersDark = window.matchMedia(
        "(prefers-color-scheme: dark)",
      ).matches;
      setEffectiveTheme(prefersDark ? "dark" : "light");
    } else {
      setEffectiveTheme(currentTheme);
    }
  };

  const setTheme = (newTheme: Theme) => {
    setThemeState(newTheme);
    localStorage.setItem("octofhir-theme", newTheme);
  };

  onMount(() => {
    updateEffectiveTheme();

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = () => updateEffectiveTheme();
    mediaQuery.addEventListener("change", handler);

    return () => mediaQuery.removeEventListener("change", handler);
  });

  createEffect(() => {
    const effective = effectiveTheme();
    document.documentElement.setAttribute("data-theme", effective);
  });

  createEffect(() => {
    theme();
    updateEffectiveTheme();
  });

  return (
    <ThemeContext.Provider value={{ theme, setTheme, effectiveTheme }}>
      {props.children}
    </ThemeContext.Provider>
  );
};

export const useTheme = () => {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error("useTheme must be used within ThemeProvider");
  }
  return context;
};
