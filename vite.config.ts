import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  base: "./",
  plugins: [tailwindcss()],
  resolve: { tsconfigPaths: true },
  build: { target: "es2023" },
  test: { include: ["tests/*.test.ts"] },
});
