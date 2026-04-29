import type { Preview, ReactRenderer } from "@storybook/react-vite";
import { withThemeByDataAttribute } from "@storybook/addon-themes";
import { ColorSchemeProvider } from "../src/shared/theme/color-scheme";
import { UIProviderInner } from "../src/app/UIProvider";

// Single source of truth for Gravity + Octo styles.
import "../src/styles";

const preview: Preview = {
    parameters: {
        controls: {
            matchers: {
                color: /(background|color)$/i,
                date: /Date$/i,
            },
        },
        layout: "padded",
        docs: { toc: true },
        backgrounds: { disable: true },
    },
    decorators: [
        (Story, context) => {
            const colorScheme = (context.globals.theme as "light" | "dark") || "light";
            return (
                <ColorSchemeProvider colorScheme={colorScheme} persist={false}>
                    <UIProviderInner>
                        <div
                            style={{
                                minHeight: "100vh",
                                padding: 16,
                                background: "var(--g-color-base-background)",
                                color: "var(--g-color-text-primary)",
                            }}
                        >
                            <Story />
                        </div>
                    </UIProviderInner>
                </ColorSchemeProvider>
            );
        },
        withThemeByDataAttribute<ReactRenderer>({
            themes: { light: "light", dark: "dark" },
            defaultTheme: "light",
            attributeName: "data-theme",
        }),
    ],
    tags: ["autodocs"],
};

export default preview;
