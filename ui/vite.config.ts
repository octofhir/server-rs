import { resolve } from "node:path";
import solid from "vite-plugin-solid";
import { defineConfig } from "vite";

export default defineConfig({
	plugins: [solid()],
	base: "/ui/",
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
		include: ["monaco-editor"],
	},
});
