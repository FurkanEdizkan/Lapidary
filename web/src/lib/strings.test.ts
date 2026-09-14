import { expect, test } from 'vitest'
import { strings } from './strings'

test('a finished batch reads as it always did when nothing was revised or declined', () => {
  expect(strings.scan.finished(6, 0)).toBe('Scan complete — 6 added.')
  expect(strings.scan.finished(3, 2)).toBe('Scan complete — 3 added, 2 already here.')
  expect(strings.upload.batchFinished(1, 1)).toBe('Upload complete — 1 added, 1 already here.')
})

test('a revised file is counted, and a change a hobby library did not keep is explained', () => {
  expect(strings.scan.finished(0, 4, 1)).toBe('Scan complete — 0 added, 1 revised, 4 already here.')
  expect(strings.scan.finished(0, 5, 0, 1)).toBe(
    'Scan complete — 0 added, 5 already here. 1 file changed but was not kept: this library keeps no revisions. Switch it to keep every change, or give a file a new name to add it as a new part.',
  )
  expect(strings.upload.batchFinished(0, 0, 0, 2)).toContain(
    '2 files changed but were not kept',
  )
})
