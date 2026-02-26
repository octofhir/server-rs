import type { Preview, ReactRenderer } from "@storybook/react";
import { withThemeByDataAttribute } from "@storybook/addon-themes";
import { MantineProvider, type MantineColorScheme } from "@mantine/core";
import { theme, resolver } from "../src/shared/theme";

import "@mantine/core/styles.layer.css";
import "@mantine/dates/styles.layer.css";
import "@mantine/notifications/styles.layer.css";

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
      const colorScheme = (context.globals.theme ||
        "light") as MantineColorScheme;
      return (
        <MantineProvider
          theme={theme}
          cssVariablesResolver={resolver}
          defaultColorScheme={colorScheme}
          forceColorScheme={colorScheme}
        >
          <Story />
        </MantineProvider>
      );
    },
    withThemeByDataAttribute<ReactRenderer>({
      themes: {
        light: "light",
        dark: "dark",
      },
      defaultTheme: "light",
      attributeName: "data-mantine-color-scheme",
    }),
  ],
  tags: ["autodocs"],
};

export default preview;
