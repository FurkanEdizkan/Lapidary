/*
  The component surface Lapidary hands to claude.ai/design.

  Re-exports only. The design agent builds with the real shipped components — this file
  chooses which ones it can see, and nothing else. Never add an implementation here.

  It sits outside `src/` deliberately: tsconfig's `include` is `["src", "vite.config.ts",
  "vitest.config.ts"]`, so `npm run build` never typechecks it and the app build carries no
  cost for the sync. `.design-sync/config.json` points its `entry` here.

  Three kinds, by what they need around them:
  - nothing: `Icon`, `Menu`, `Dialog`, `Grid`, `GridSkeleton`, `SelectionBar`, `Figure`,
    `MeasureBar`, `SectionBar`, `FirstRun`;
  - a router: `AppFrame`, `Card`, `Crash` — wrap them in `DesignProviders`;
  - Lapidary's API: `FolderTree`, `MovePartDialog`, `Detail`, `ShareDialog` — they render inside
    `DesignProviders` too, and show their loading state, since a design has no server.
*/
export { DesignProviders } from './.ds-providers'

export { Icon } from './src/components/Icon'
export { Menu } from './src/components/Menu'
export { Dialog } from './src/components/Dialog'
export { Grid, GridSkeleton, SelectionBar } from './src/components/Grid'
export { Figure } from './src/components/Figure'
export { MeasureBar, SectionBar } from './src/components/Measure'
export { FirstRun } from './src/components/FirstRun'

export { AppFrame } from './src/components/AppFrame'
export { Card } from './src/components/Card'
export { Crash } from './src/components/Crash'

export { FolderTree, MovePartDialog } from './src/components/FolderTree'
export { Detail } from './src/components/PartDetail'
export { ShareDialog } from './src/components/ShareDialog'
