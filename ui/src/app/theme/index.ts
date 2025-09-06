import { createTheme, type MantineColorsTuple, virtualColor } from "@mantine/core";
import logoUrl from "@/shared/assets/logo.png";
import { extractPalette } from "@/shared/lib/palette";

// Extract colors from logo and create theme palette
let primaryColors: MantineColorsTuple = [
  "#e3f2fd",
  "#bbdefb",
  "#90caf9",
  "#64b5f6",
  "#42a5f5",
  "#2196f3",
  "#1e88e5",
  "#1976d2",
  "#1565c0",
  "#0d47a1",
];

// This will be populated asynchronously
export let themeColors: Record<string, MantineColorsTuple> = {};

export async function initializeTheme() {
  try {
    const palette = await extractPalette(logoUrl);
    primaryColors = palette.primary as unknown as MantineColorsTuple;
    // Only apply primary to Mantine palette for now to avoid type mismatches
    // (Mantine expects 10 shades; ensure tuple has length 10)
    updateThemeColors({
      primary: ensureMantineTuple(palette.primary),
    });
  } catch (error) {
    console.warn("Failed to extract colors from logo, using defaults:", error);
  }
}

export const theme = createTheme({
  primaryColor: "primary",
  colors: {
    primary: primaryColors,
    // Virtual colors that will be updated after theme initialization
    secondary: virtualColor({
      name: "secondary",
      dark: "gray",
      light: "gray",
    }),
    accent: virtualColor({
      name: "accent",
      dark: "blue",
      light: "blue",
    }),
  },
  fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif",
  fontFamilyMonospace: "JetBrains Mono, Monaco, Consolas, monospace",
  headings: {
    fontFamily: "Inter, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, sans-serif",
    fontWeight: "600",
  },
  radius: {
    xs: "4px",
    sm: "6px",
    md: "8px",
    lg: "12px",
    xl: "16px",
  },
  spacing: {
    xs: "8px",
    sm: "12px",
    md: "16px",
    lg: "24px",
    xl: "32px",
  },
  breakpoints: {
    xs: "36em",
    sm: "48em",
    md: "62em",
    lg: "75em",
    xl: "88em",
  },
  components: {
    Button: {
      defaultProps: {
        radius: "md",
      },
    },
    TextInput: {
      defaultProps: {
        radius: "md",
      },
    },
    Card: {
      defaultProps: {
        radius: "lg",
        shadow: "sm",
      },
    },
    Modal: {
      defaultProps: {
        radius: "lg",
        shadow: "xl",
      },
    },
    Paper: {
      defaultProps: {
        radius: "lg",
      },
    },
  },
});

// Function to update theme with extracted colors
export function updateThemeColors(colors: Record<string, MantineColorsTuple>) {
  const current = (theme as any).colors || {};
  for (const key of Object.keys(colors)) {
    current[key] = colors[key];
  }
  (theme as any).colors = current;
}

// Helper to ensure palette has 10 shades for Mantine
export function ensureMantineTuple(colors: string[]): MantineColorsTuple {
  if (colors.length === 10) return colors as unknown as MantineColorsTuple;
  if (colors.length > 10) return colors.slice(0, 10) as unknown as MantineColorsTuple;
  // If we have 9 (common), duplicate the last shade to make 10
  if (colors.length === 9) return [...colors, colors[colors.length - 1]] as unknown as MantineColorsTuple;
  // Fallback: pad by repeating last available color
  const padded = [...colors];
  while (padded.length < 10) padded.push(padded[padded.length - 1] ?? "#1976d2");
  return padded as unknown as MantineColorsTuple;
}