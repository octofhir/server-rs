import { createTheme, type MantineColorsTuple, virtualColor } from "@mantine/core";

// Primary color palette - Purple to match OctoFHIR octopus logo
const primary: MantineColorsTuple = [
	"#faf5ff", // 0 - lightest
	"#f3e8ff", // 1
	"#e9d5ff", // 2
	"#d8b4fe", // 3
	"#c084fc", // 4 - light purple
	"#a855f7", // 5 - purple-500
	"#9333ea", // 6 - primary (main) - matches octopus
	"#7e22ce", // 7 - hover
	"#6b21a8", // 8
	"#581c87", // 9 - darkest
];

// Fire/flame accent colors (orange-red from logo)
const fire: MantineColorsTuple = [
	"#fff7ed", // 0
	"#ffedd5", // 1
	"#fed7aa", // 2
	"#fdba74", // 3
	"#fb923c", // 4
	"#f97316", // 5 - orange
	"#ea580c", // 6 - main fire
	"#c2410c", // 7
	"#9a3412", // 8
	"#7c2d12", // 9
];

// Gray palette for text/borders
const gray: MantineColorsTuple = [
	"#f7f8fa", // 0 - bg-secondary
	"#f2f3f5", // 1 - bg-tertiary
	"#eff2f5", // 2 - border-subtle
	"#d0d7de", // 3 - border-color
	"#afb8c1", // 4 - text-subtle
	"#8c959f", // 5 - text-muted
	"#656d76", // 6 - text-secondary
	"#1f2328", // 7 - text-color
	"#161618", // 8 - dark bg
	"#0d0d0e", // 9 - darkest
];

export const theme = createTheme({
	primaryColor: "primary",
	colors: {
		primary,
		fire,
		gray,
		// Virtual colors for semantic usage
		success: virtualColor({
			name: "success",
			dark: "green",
			light: "green",
		}),
		warning: virtualColor({
			name: "warning",
			dark: "yellow",
			light: "yellow",
		}),
		error: virtualColor({
			name: "error",
			dark: "red",
			light: "red",
		}),
	},

	// Typography
	fontFamily: '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
	fontFamilyMonospace: '"JetBrains Mono", "SF Mono", "Fira Code", Consolas, monospace',

	fontSizes: {
		xs: "0.75rem", // 12px
		sm: "0.8125rem", // 13px
		md: "0.875rem", // 14px
		lg: "1rem", // 16px
		xl: "1.125rem", // 18px
	},

	// Spacing scale
	spacing: {
		xs: "0.25rem", // 4px
		sm: "0.5rem", // 8px
		md: "1rem", // 16px
		lg: "1.5rem", // 24px
		xl: "2rem", // 32px
	},

	// Border radius
	radius: {
		xs: "4px",
		sm: "6px",
		md: "8px",
		lg: "12px",
		xl: "16px",
	},

	// Shadows
	shadows: {
		xs: "0 1px 2px rgba(0, 0, 0, 0.04)",
		sm: "0 1px 3px rgba(0, 0, 0, 0.08), 0 1px 2px rgba(0, 0, 0, 0.04)",
		md: "0 4px 8px rgba(0, 0, 0, 0.08), 0 2px 4px rgba(0, 0, 0, 0.04)",
		lg: "0 12px 24px rgba(0, 0, 0, 0.1), 0 4px 8px rgba(0, 0, 0, 0.04)",
		xl: "0 20px 40px rgba(0, 0, 0, 0.12), 0 8px 16px rgba(0, 0, 0, 0.06)",
	},

	// Layout constants
	other: {
		headerHeight: 56,
		sidebarWidth: 240,
		sidebarCollapsedWidth: 64,
		contentMaxWidth: 1400,
	},

	// Default component styles
	components: {
		Button: {
			defaultProps: {
				radius: "sm",
			},
		},
		Card: {
			defaultProps: {
				radius: "md",
				withBorder: true,
			},
		},
		Paper: {
			defaultProps: {
				radius: "md",
			},
		},
		TextInput: {
			defaultProps: {
				radius: "sm",
			},
		},
		Select: {
			defaultProps: {
				radius: "sm",
			},
		},
		Modal: {
			defaultProps: {
				radius: "lg",
				centered: true,
			},
		},
		Notification: {
			defaultProps: {
				radius: "md",
			},
		},
		Badge: {
			defaultProps: {
				radius: "sm",
			},
		},
		Table: {
			defaultProps: {
				striped: true,
				highlightOnHover: true,
			},
		},
	},
});
