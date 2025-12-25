import { useEffect } from "react";
import { useMantineColorScheme, useMantineTheme } from "@mantine/core";

type CssVarMap = Record<string, string>;

type TokenGroup = {
	prefix: string;
	values: Record<string, string>;
};

function toKebabCase(value: string) {
	return value.replace(/[A-Z]/g, (match) => `-${match.toLowerCase()}`);
}

function resolveScheme(colorScheme: string) {
	if (colorScheme === "auto") {
		const dataScheme =
			document.documentElement.getAttribute("data-mantine-color-scheme");
		if (dataScheme === "dark" || dataScheme === "light") return dataScheme;
		return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
	}
	return colorScheme;
}

function applyCssVars(root: HTMLElement, vars: CssVarMap) {
	Object.entries(vars).forEach(([key, value]) => {
		root.style.setProperty(key, value);
	});
}

function buildCssVars(groups: TokenGroup[]) {
	return groups.reduce<CssVarMap>((acc, group) => {
		Object.entries(group.values).forEach(([key, value]) => {
			acc[`${group.prefix}-${toKebabCase(key)}`] = value;
		});
		return acc;
	}, {});
}

export function ThemeCssVars() {
	const theme = useMantineTheme();
	const { colorScheme } = useMantineColorScheme();

	useEffect(() => {
		const root = document.documentElement;
		const scheme = resolveScheme(colorScheme) as "light" | "dark";

		const baseVars = buildCssVars([
			{
				prefix: "--app-brand",
				values: theme.other.brand as unknown as Record<string, string>,
			},
		]);

		const schemeVars: Record<"light" | "dark", CssVarMap> = {
			light: {
				...buildCssVars([
					{
						prefix: "--app-surface",
						values: {
							"1": theme.white,
							"2": theme.colors.gray[0],
							"3": theme.colors.gray[1],
						},
					},
					{
						prefix: "--app-accent",
						values: {
							primary: theme.other.brand.primary,
							warm: theme.other.brand.warm,
							fire: theme.other.brand.fire,
							warmBg: theme.colors.warm[0],
							fireBg: theme.colors.fire[0],
						},
					},
					{
						prefix: "--app-text",
						values: {
							primary: theme.colors.gray[9],
							secondary: theme.colors.gray[6],
						},
					},
					{
						prefix: "--app-border",
						values: {
							subtle: theme.colors.gray[2],
						},
					},
				]),
				"--app-glass-bg": theme.white,
				"--app-glass-border": "transparent",
				"--app-glass-blur": "0px",
				"--app-header-bg": theme.white,
				"--app-header-fg": theme.colors.gray[9],
				"--app-header-hover-bg": theme.colors.gray[0],
				"--app-header-badge-bg": theme.colors.gray[1],
				"--app-login-bg": theme.other.brand.gradient,
				"--app-login-panel-bg": theme.white,
			},
			dark: {
				...buildCssVars([
					{
						prefix: "--app-surface",
						values: {
							"1": "#0d0e1a", // Deep space background
							"2": "#141629", // Slightly lighter surface
							"3": "#1c1f40", // Elevated surface
						},
					},
					{
						prefix: "--app-accent",
						values: {
							primary: theme.other.brand.primaryLight,
							warm: theme.other.brand.warm,
							fire: theme.other.brand.fire,
							warmBg: "rgba(168, 132, 120, 0.15)",
							fireBg: "rgba(255, 77, 61, 0.15)",
						},
					},
					{
						prefix: "--app-text",
						values: {
							primary: "#f8f9fe",
							secondary: theme.colors.gray[4],
						},
					},
					{
						prefix: "--app-border",
						values: {
							subtle: "rgba(255, 255, 255, 0.08)",
						},
					},
				]),
				"--app-glass-bg": "rgba(13, 14, 26, 0.7)",
				"--app-glass-border": "rgba(255, 255, 255, 0.08)",
				"--app-glass-blur": "16px",
				"--app-header-bg": "rgba(13, 14, 26, 0.8)",
				"--app-header-fg": "#f8f9fe",
				"--app-header-hover-bg": "rgba(255, 255, 255, 0.06)",
				"--app-header-badge-bg": "rgba(255, 255, 255, 0.04)",
				"--app-login-bg": "radial-gradient(circle at top left, #1c1f40, #0d0e1a)",
				"--app-login-panel-bg": "rgba(20, 22, 41, 0.8)",
			},
		};

		applyCssVars(root, { ...baseVars, ...schemeVars[scheme] });
	}, [colorScheme, theme]);

	return null;
}
