import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    files: ["src/**/*.ts"],
    ignores: ["src/**/*.test.ts"],
    languageOptions: {
      parser: tseslint.parser,
    },
    rules: {
      complexity: ["error", 15],
      "@typescript-eslint/no-explicit-any": "off",
    },
  },
);
