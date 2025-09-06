import { ColorSchemeScript, MantineProvider } from "@mantine/core";
import { useEffect, useState } from "react";
import { useUnit } from "effector-react";
import { initializeTheme, theme } from "../theme";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import { $colorScheme } from "@/entities/settings/model";

interface ThemeProviderProps {
  children: React.ReactNode;
}

export function ThemeProvider({ children }: ThemeProviderProps) {
  const colorScheme = useUnit($colorScheme);
  const [themeInitialized, setThemeInitialized] = useState(false);

  // Initialize theme colors from logo
  useEffect(() => {
    initializeTheme()
      .then(() => {
        setThemeInitialized(true);
      })
      .catch((error) => {
        console.warn("Theme initialization failed:", error);
        setThemeInitialized(true);
      });
  }, []);

  if (!themeInitialized) {
    return (
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          height: "100vh",
          fontFamily: "Inter, sans-serif",
          fontSize: "14px",
          color: "#666",
        }}
      >
        Initializing theme...
      </div>
    );
  }

  // Mantine v7: use defaultColorScheme for initial value and forceColorScheme to override
  const force = colorScheme === "auto" ? undefined : colorScheme;

  return (
    <>
      <ColorSchemeScript defaultColorScheme="auto" />
      <MantineProvider theme={theme} defaultColorScheme="auto" forceColorScheme={force}>
        {children}
      </MantineProvider>
    </>
  );
}
