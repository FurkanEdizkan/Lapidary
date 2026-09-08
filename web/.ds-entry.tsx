/*
  The component surface Lapidary hands to claude.ai/design.

  Re-exports only. The design agent builds with the real shipped components — this file
  chooses which ones it can see, and nothing else. Never add an implementation here.

  It sits outside `src/` deliberately: tsconfig's `include` is `["src", "vite.config.ts",
  "vitest.config.ts"]`, so `npm run build` never typechecks it and the app build carries no
  cost for the sync. `.design-sync/config.json` points its `entry` here.
*/
export { Dialog } from './src/components/Dialog'
export { FolderTree, MovePartDialog } from './src/components/FolderTree'
export { Detail } from './src/components/PartDetail'
