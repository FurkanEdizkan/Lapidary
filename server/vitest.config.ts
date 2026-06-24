import { defineConfig } from 'vitest/config';

export default defineConfig({
  // The codebase uses NodeNext-style `.js` import specifiers that point at `.ts`
  // sources; map them so Vitest can resolve them.
  resolve: { extensionAlias: { '.js': ['.ts', '.js'] } },
  test: {
    environment: 'node',
    include: ['test/**/*.test.ts'],
    // Tests that use the getDb() singleton share one file DB; run files
    // sequentially so they don't clobber each other.
    fileParallelism: false,
    // better-sqlite3 is a native addon — let Node load it, don't let Vite transform it.
    server: { deps: { external: ['better-sqlite3'] } },
  },
});
