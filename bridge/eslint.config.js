// SPDX-License-Identifier: GPL-3.0-or-later
import js from '@eslint/js';
import tseslint from 'typescript-eslint';

export default tseslint.config(
    {ignores: ['build/', 'node_modules/', 'assemble.mjs', 'eslint.config.js']},
    js.configs.recommended,
    ...tseslint.configs.recommended,
    {
        rules: {
            '@typescript-eslint/no-unused-vars': ['error', {argsIgnorePattern: '^_'}],
        },
    },
);
