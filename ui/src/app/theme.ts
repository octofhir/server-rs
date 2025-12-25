import { createTheme, type MantineColorsTuple, virtualColor, type MantineTheme, Button, Badge, TextInput, ActionIcon, Modal } from "@mantine/core";
import buttonClasses from "@/shared/ui/Button/Button.module.css";
import badgeClasses from "@/shared/ui/Badge/Badge.module.css";
import textInputClasses from "@/shared/ui/TextInput/TextInput.module.css";
import actionIconClasses from "@/shared/ui/ActionIcon/ActionIcon.module.css";
import cardClasses from "@/shared/ui/Card/Card.module.css";

export interface BrandTokens {
	primary: string;
	primaryLight: string;
	primaryDark: string;
	deep: string;
	fire: string;
	warm: string;
	gradient: string;
}

export interface SemanticTokens {
	surface1: string;
	surface2: string;
	surface3: string;
	textPrimary: string;
	textSecondary: string;
	borderSubtle: string;
	accentPrimary: string;
	accentWarm: string;
	accentFire: string;
	accentWarmBg: string;
	accentFireBg: string;
}

// Primary palette derived from logo blue - refined for 2025
const primary: MantineColorsTuple = [
	"#f0f4ff", // 0
	"#d9e2ff", // 1
	"#b0c3ff", // 2
	"#87a4ff", // 3
	"#5e85ff", // 4
	"#3b3fe3", // 5 - vibrant primary
	"#2f32c1", // 6
	"#24269f", // 7
	"#191b7d", // 8
	"#0f105b", // 9
];

// Accent palette derived from logo orange - more vibrant
const fire: MantineColorsTuple = [
	"#fff1f0", // 0
	"#ffe3e1", // 1
	"#ffc6c2", // 2
	"#ffa198", // 3
	"#ff7a6d", // 4
	"#ff4d3d", // 5 - vibrant fire
	"#d9362a", // 6
	"#b3261b", // 7
	"#8d180f", // 8
	"#680d05", // 9
];

const deep: MantineColorsTuple = [
	"#f8f9fe",
	"#ebedf7",
	"#d4d8ef",
	"#adb3df",
	"#828bcd",
	"#5c66b5",
	"#49519c",
	"#3a407e",
	"#2b305f",
	"#1c1f40", // 9 - very dark for backgrounds
];

const warm: MantineColorsTuple = [
	"#fdfaf9",
	"#f7efed",
	"#ede0db",
	"#dfccc5",
	"#cfb5ac",
	"#bc9d92",
	"#a88478",
	"#8d6b5e",
	"#72554a",
	"#574038",
];

// Gray palette for text/borders - refined for softer UI
const gray: MantineColorsTuple = [
	"#f8f9fa", // 0
	"#f1f3f5", // 1
	"#e9ecef", // 2
	"#dee2e6", // 3
	"#ced4da", // 4
	"#adb5bd", // 5
	"#868e96", // 6
	"#495057", // 7
	"#343a40", // 8
	"#212529", // 9
];

export const theme = createTheme({
	primaryColor: "primary",
	primaryShade: { light: 5, dark: 4 },
	defaultGradient: { from: "primary.5", to: "fire.5", deg: 135 },
	colors: {
		primary,
		fire,
		deep,
		warm,
		gray,
		// Virtual colors for semantic usage
		success: virtualColor({
			name: "success",
			dark: "primary",
			light: "primary",
		}),
		warning: virtualColor({
			name: "warning",
			dark: "warm",
			light: "warm",
		}),
		error: virtualColor({
			name: "error",
			dark: "fire",
			light: "fire",
		}),
	},

	// Typography
	fontFamily: '"Inter", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
	fontFamilyMonospace: '"JetBrains Mono", "SF Mono", "Fira Code", Consolas, monospace',

	headings: {
		fontWeight: "600",
		fontFamily: '"Outfit", "Inter", sans-serif',
	},

	fontSizes: {
		xs: "0.75rem", // 12px
		sm: "0.875rem", // 14px
		md: "1rem", // 16px
		lg: "1.125rem", // 18px
		xl: "1.25rem", // 20px
	},

	// Spacing scale
	spacing: {
		xs: "0.5rem", // 8px
		sm: "0.75rem", // 12px
		md: "1.25rem", // 20px
		lg: "2rem", // 32px
		xl: "3rem", // 48px
	},

	// Border radius - Refined for 2025 "Linear" precision
	radius: {
		xs: "2px",
		sm: "4px",
		md: "6px",
		lg: "8px",
		xl: "12px",
	},

	// Shadows - Subtler for Linear-like precision
	shadows: {
		xs: "0 1px 2px rgba(0, 0, 0, 0.04)",
		sm: "0 1px 3px rgba(0, 0, 0, 0.05), 0 1px 2px rgba(0, 0, 0, 0.1)",
		md: "0 4px 6px -1px rgba(0, 0, 0, 0.1), 0 2px 4px -1px rgba(0, 0, 0, 0.06)",
		lg: "0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05)",
		xl: "0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04)",
	},

	// Layout constants
	other: {
		headerHeight: 48,
		sidebarWidth: 240,
		sidebarCollapsedWidth: 64,
		contentMaxWidth: 1400,
		brand: {
			primary: "#3b3fe3",
			primaryLight: "#5e85ff",
			primaryDark: "#0f105b",
			deep: "#1c1f40",
			fire: "#ff4d3d",
			warm: "#a88478",
			gradient: "linear-gradient(135deg, #3b3fe3 0%, #ff4d3d 100%)",
		} satisfies BrandTokens,
		semantic: {
			surface1: "var(--app-surface-1)",
			surface2: "var(--app-surface-2)",
			surface3: "var(--app-surface-3)",
			textPrimary: "var(--app-text-primary)",
			textSecondary: "var(--app-text-secondary)",
			borderSubtle: "var(--app-border-subtle)",
			accentPrimary: "var(--app-accent-primary)",
			accentWarm: "var(--app-accent-warm)",
			accentFire: "var(--app-accent-fire)",
			accentWarmBg: "var(--app-accent-warm-bg)",
			accentFireBg: "var(--app-accent-fire-bg)",
		} satisfies SemanticTokens,
	},

	// Default component styles
	components: {
		Button: Button.extend({
			classNames: buttonClasses,
			defaultProps: {
				radius: "md",
				color: "primary",
				size: "sm",
				variant: "filled",
			},
		}),
		Card: {
			defaultProps: {
				radius: "lg",
				shadow: "sm",
				padding: "md",
			},
			classNames: cardClasses,
		},
		Paper: {
			defaultProps: {
				radius: "lg",
				shadow: "xs",
			},
			styles: {
				root: {
					backgroundColor: "var(--app-surface-1)",
				},
			},
		},
		Table: {
			defaultProps: {
				striped: false,
				highlightOnHover: true,
				verticalSpacing: "sm",
			},
			styles: {
				tr: {
					transition: "background-color 100ms ease",
				},
			},
		},
		TextInput: TextInput.extend({
			classNames: textInputClasses,
			defaultProps: {
				radius: "md",
				size: "sm",
			},
		}),
		ActionIcon: ActionIcon.extend({
			classNames: actionIconClasses,
			defaultProps: {
				variant: "subtle",
				radius: "md",
				size: "md",
			},
		}),
		Select: {
			defaultProps: {
				radius: "md",
				size: "sm",
			},
			styles: (theme: MantineTheme) => ({
				input: {
					backgroundColor: "var(--app-surface-2)",
					border: "1px solid var(--app-glass-border)",
				},
				label: {
					fontWeight: 600,
					marginBottom: 4,
					fontSize: theme.fontSizes.xs,
					color: "var(--app-text-secondary)",
				},
			}),
		},
		Modal: Modal.extend({
			defaultProps: {
				radius: "xl",
				centered: true,
				overlayProps: {
					blur: 8,
					opacity: 0.4,
				},
			},
		}),
		Badge: Badge.extend({
			classNames: badgeClasses,
			defaultProps: {
				radius: "sm",
				variant: "light",
			},
		}),
	},
});
