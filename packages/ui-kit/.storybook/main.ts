import type { StorybookConfig } from "@storybook/react-vite";
import path from "node:path";

const config: StorybookConfig = {
  stories: [
    "../src/**/*.mdx",
    "../src/**/*.stories.@(ts|tsx)",
  ],

  addons: [
    "@storybook/addon-docs",
    "@storybook/addon-a11y",
    "@storybook/addon-themes",
  ],

  framework: {
    name: "@storybook/react-vite",
    options: {},
  },

  // Brand assets (logo, favicon) come from the console so there is one source of truth.
  staticDirs: [{ from: "../../../ui/public/branding", to: "/branding" }],

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

  typescript: {
    reactDocgen: "react-docgen-typescript",
    reactDocgenTypescriptOptions: {
      shouldExtractLiteralValuesFromEnum: true,
      propFilter: (prop) =>
        prop.parent
          ? !/node_modules\/(?!@gravity-ui)/.test(prop.parent.fileName)
          : true,
    },
  }
};

export default config;
