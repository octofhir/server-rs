/// <reference types="vitest" />

import { resolve } from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
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
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: true,
  },
});
