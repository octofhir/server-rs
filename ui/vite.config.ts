import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
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
    port: 3000,
    proxy: {
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      // Proxy FHIR endpoints to backend
      "/Patient": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      "/Practitioner": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      "/Organization": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
      "/metadata": {
        target: "http://localhost:8080",
        changeOrigin: true,
      },
    },
  },
  css: {
    modules: {
      localsConvention: "camelCaseOnly",
    },
  },
});
