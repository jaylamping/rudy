// ESLint v9 flat config for the link/ frontend.
//
// Goals (deliberately narrow):
//   1. Make `npm run lint` actually run something (CI was failing because v9
//      defaults to flat config and we shipped no eslint.config.js).
//   2. Type-aware-ish: parse .ts/.tsx via @typescript-eslint without turning
//      on the costly project-aware rules — keeps CI fast and avoids needing
//      a separate tsconfig for ESLint.
//   3. React 19 + Vite hot-reload + TanStack Router conventions: keep the
//      hooks rules on (real correctness gate), keep react-refresh on (warns
//      when a fast-refresh boundary breaks), turn `react-in-jsx-scope` off
//      (not needed under the modern JSX transform).
//   4. Don't suddenly turn the codebase red. Anything noisier than the
//      typescript-eslint "recommended" set is opt-in.
//
// Files we explicitly do NOT lint:
//   - `dist/`, `node_modules/`            : build/output
//   - `src/routeTree.gen.ts`              : TanStack Router codegen
//   - `src/lib/types/**`                  : ts-rs Rust -> TS codegen
//   - `coverage/`, `.vite/`               : tool caches

import js from "@eslint/js";
import tsParser from "@typescript-eslint/parser";
import tsPlugin from "@typescript-eslint/eslint-plugin";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import globals from "globals";

export default [
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "coverage/**",
      ".vite/**",
      "src/routeTree.gen.ts",
      "src/lib/types/**",
    ],
  },

  js.configs.recommended,

  {
    files: ["**/*.{ts,tsx}"],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: "latest",
        sourceType: "module",
        ecmaFeatures: { jsx: true },
      },
      globals: {
        ...globals.browser,
        ...globals.es2022,
      },
    },
    plugins: {
      "@typescript-eslint": tsPlugin,
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    rules: {
      ...tsPlugin.configs.recommended.rules,
      ...reactHooks.configs.recommended.rules,

      // `no-undef` was designed for plain JS — it doesn't know the global DOM
      // type names (`RequestInit`, `ReadableStreamReadResult`, …) and false-
      // positives all over our fetch/WebTransport code. TypeScript itself
      // catches genuinely-undefined identifiers with full type fidelity, so
      // turning the rule off for .ts/.tsx loses no real signal.
      "no-undef": "off",

      // The base no-unused-vars fires on TS-only constructs (interface props,
      // type-only imports) — let the @typescript-eslint version handle it,
      // matching tsconfig's `noUnusedLocals`/`noUnusedParameters`.
      "no-unused-vars": "off",
      "@typescript-eslint/no-unused-vars": [
        "warn",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],

      // Vite Fast Refresh: only allow component exports from a route/component
      // file. Constants/utilities co-exported from those files break HMR.
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
    },
  },

  {
    // Test files: relax the `any`/`non-null-assertion` clamp because mocks
    // and test fixtures legitimately use them.
    files: ["**/*.test.{ts,tsx}", "**/__tests__/**/*.{ts,tsx}"],
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
        ...globals.vitest,
      },
    },
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-non-null-assertion": "off",
    },
  },

  {
    // Build/config scripts run under Node.
    files: [
      "vite.config.ts",
      "vitest.config.ts",
      "scripts/**/*.{js,mjs,cjs}",
      "eslint.config.js",
    ],
    languageOptions: {
      globals: {
        ...globals.node,
      },
    },
  },
];
