import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
// @ts-ignore - CommonJS module compatibility
import monacoEditorPluginModule from "vite-plugin-monaco-editor";

const monacoEditorPlugin =
	monacoEditorPluginModule.default || monacoEditorPluginModule;

// React is now the primary framework
// Monaco workers are configured via vite-plugin-monaco-editor

export default defineConfig({
	base: "/ui/",
	plugins: [
		react(),
		monacoEditorPlugin({
			languageWorkers: ["editorWorkerService", "json", "typescript"],
			customWorkers: [
				{
					label: "graphql",
					entry: "monaco-graphql/esm/graphql.worker",
				},
			],
		}),
	],
	resolve: {
		alias: {
			"@": resolve(__dirname, "./src"),
			"@/app": resolve(__dirname, "./src/app"),
			"@/app-react": resolve(__dirname, "./src/app-react"),
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
		include: [
			"react",
			"react-dom",
			"graphiql",
			"@graphiql/react",
			"monaco-editor",
			"monaco-graphql",
		],
	},
	worker: {
		format: "es",
	},
});
