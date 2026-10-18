import { vi } from 'vitest'

/*
  Every page test mounts the real route tree, whose root asks the session gate before any page (A1).
  Those tests are about their page, and each mocks `fetch` for the calls that page makes; a signed-in
  person is the ground they stand on, so the gate lets them through here. `session.test.ts` tests the
  gate itself, with `vi.importActual`.
*/
vi.mock('./lib/session', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./lib/session')>()),
  gate: async () => null,
}))

/*
  Motion's animations finish at once in tests (A18): a step that has to leave before the next one mounts would
  otherwise hold both in the DOM for the length of its exit.
*/
import { MotionGlobalConfig } from 'motion/react'
MotionGlobalConfig.skipAnimations = true
