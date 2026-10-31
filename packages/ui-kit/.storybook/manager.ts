import { addons } from "storybook/manager-api";
import { create } from "storybook/theming";

// Storybook chrome in the OctoFHIR brand: graphite neutrals, one Marigold accent.
const theme = create({
  base: "dark",
  brandTitle: "OctoFHIR UI Kit",
  brandUrl: "https://github.com/octofhir/server-rs",
  brandImage: "branding/logo-dark.svg",
  brandTarget: "_blank",
  colorPrimary: "#f2b630",
  colorSecondary: "#f2b630",
  appBg: "#18191c",
  appContentBg: "#111214",
  appPreviewBg: "#111214",
  appBorderColor: "#26272b",
  appBorderRadius: 5,
  barBg: "#18191c",
  barTextColor: "#8b8b93",
  barSelectedColor: "#f7c95a",
  textColor: "#ededef",
  textMutedColor: "#8b8b93",
  fontBase: '"Inter", system-ui, sans-serif',
  fontCode: '"JetBrains Mono", ui-monospace, monospace',
});

addons.setConfig({
  theme,
  sidebar: {
    showRoots: true,
  },
});
