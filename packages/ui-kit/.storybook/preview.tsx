import type { Preview, ReactRenderer } from "@storybook/react-vite";
import { withThemeByDataAttribute } from "@storybook/addon-themes";
import { ThemeProvider, ToasterProvider, ToasterComponent, Toaster } from "@gravity-ui/uikit";

import "@gravity-ui/uikit/styles/fonts.css";
import "@gravity-ui/uikit/styles/styles.css";
import "../src/shared/theme/fonts.css";
import "../src/shared/theme/gravity-overrides.css";

const toaster = new Toaster();

const preview: Preview = {
  parameters: {
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    layout: "centered",
    docs: {
      toc: true,
    },
  },
  decorators: [
    (Story, context) => {
      const colorScheme = context.globals.theme || "light";
      return (
        <ThemeProvider theme={colorScheme}>
          <ToasterProvider toaster={toaster}>
            <Story />
            <ToasterComponent />
          </ToasterProvider>
        </ThemeProvider>
      );
    },
    withThemeByDataAttribute<ReactRenderer>({
      themes: {
        light: "light",
        dark: "dark",
      },
      defaultTheme: "light",
      attributeName: "data-theme",
    }),
  ],
  tags: ["autodocs"],
};

export default preview;
