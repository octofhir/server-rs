import { resolve } from "node:path";
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { monaco as monacoEditorPlugin } from "@bithero/monaco-editor-vite-plugin";

export default defineConfig({
	base: "/ui/",
	plugins: [
		solid(),
		monacoEditorPlugin({
			// Only include SQL, JavaScript, TypeScript languages
			languages: ["sql", "javascript", "typescript"],
			// Include all features for these languages
			features: "all",
			// Expose Monaco globally for our custom worker setup
			globalAPI: true,
		}),
	],
	resolve: {
		alias: {
			"@": resolve(__dirname, "./src"),
			"@/app": resolve(__dirname, "./src/app"),
			"@/processes": resolve(__dirname, "./src/processes"),
			"@/pages": resolve(__dirname, "./src/pages"),
			"@/widgets": resolve(__dirname, "./src/widgets"),
			"@/features": resolve(__dirname, "./src/features"),
			"@/entities": resolve(__dirname, "./src/entities"),
			"@/shared": resolve(__dirname, "./src/shared"),
		},
	},
	server: {
		port: 4000,
		proxy: {
			"/api": {
				target: "http://localhost:8888",
				changeOrigin: true,
				ws: true,
			},
			"/fhir": {
				target: "http://localhost:8888",
				changeOrigin: true,
			},
			"/auth": {
				target: "http://localhost:8888",
				changeOrigin: true,
			},
		},
	},
	css: {
		modules: {
			localsConvention: "camelCaseOnly",
		},
	},
	build: {
		target: "esnext",
		outDir: "dist",
	},
	optimizeDeps: {
		exclude: ["monaco-editor"],
	},
});
