// SPDX-License-Identifier: GPL-3.0-or-later
// Lint with typescript-eslint on the TypeScript 6 compiler API (the `typescript` package in
// package.json); the build itself uses TypeScript 7, see the "//" note there.
import js from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
    {ignores: ['build/', 'node_modules/', 'assemble.mjs', 'eslint.config.js']},
    js.configs.recommended,
    ...tseslint.configs.strictTypeChecked,
    ...tseslint.configs.stylisticTypeChecked,
    {
        languageOptions: {
            parserOptions: {
                projectService: true,
                tsconfigRootDir: import.meta.dirname,
            },
        },
        rules: {
            '@typescript-eslint/no-explicit-any': 'error',
            '@typescript-eslint/no-unused-vars': ['error', {argsIgnorePattern: '^_'}],
        },
    },
);
