import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { resolve } from "path";

export default defineConfig({
  plugins: [solid()],
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
      "@/shared": resolve(__dirname, "src/shared"),
      "@/entities": resolve(__dirname, "src/entities"),
      "@/features": resolve(__dirname, "src/features"),
      "@/widgets": resolve(__dirname, "src/widgets"),
      "@/pages": resolve(__dirname, "src/pages"),
      "@/app": resolve(__dirname, "src/app"),
    },
  },
  css: {
    modules: {
      localsConvention: "camelCase",
      generateScopedName: "[name]__[local]___[hash:base64:5]",
    },
  },
  server: {
    port: 3000,
    proxy: {
      "/api": {
        target: "http://localhost:8080",
        changeOrigin: true,
        secure: false,
      },
    },
  },
  build: {
    target: "esnext",
    outDir: "dist",
    sourcemap: true,
  },
});