import type { StorybookConfig } from "@storybook/react-vite";
import path from "node:path";

const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|tsx)"],
  addons: [
    "@storybook/addon-docs",
    "@storybook/addon-a11y",
    "@storybook/addon-themes",
  ],
  framework: {
    name: "@storybook/react-vite",
    options: {},
  },
  viteFinal: async (config, { configType }) => {
    config.css = {
      ...config.css,
      modules: {
        ...config.css?.modules,
        localsConvention: "camelCaseOnly" as const,
      },
    };

    config.resolve = {
      ...config.resolve,
      alias: {
        ...((config.resolve?.alias as Record<string, string>) ?? {}),
        "#": path.resolve(import.meta.dirname, "../src"),
      },
    };

    if (configType === "BUILD") {
      config.base = "/server-rs/storybook/";
    }

    return config;
  },
  docs: {
    autodocs: "tag",
  },
  typescript: {
    reactDocgen: "react-docgen-typescript",
    reactDocgenTypescriptOptions: {
      shouldExtractLiteralValuesFromEnum: true,
      propFilter: (prop) =>
        prop.parent
          ? !/node_modules\/(?!@mantine)/.test(prop.parent.fileName)
          : true,
    },
  },
};

export default config;
