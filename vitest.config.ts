import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    coverage: {
      provider: "v8",
      reporter: ["text", "json-summary"],
      reportsDirectory: "./coverage",
      // `all: true` reports on every file under `include`, not just files
      // imported by a test. Keeps main.ts honest.
      all: true,
      include: ["src/**/*.ts"],
      exclude: [
        "src/**/*.test.ts",
        "src/**/*.d.ts",
        "src/main.ts", // wired via DOM events; integration-tested via UI
        "src/types.ts", // pure type declarations, no runtime
        ".covid/**",
        "node_modules/**",
        "dist/**",
        "src-tauri/**",
      ],
    },
    exclude: [".covid/**", "node_modules/**", "dist/**", "src-tauri/**"],
  },
});
