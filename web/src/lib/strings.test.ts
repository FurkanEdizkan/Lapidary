import { expect, test } from 'vitest'
import { strings } from './strings'

test('a finished batch reads as it always did when nothing was revised or declined', () => {
  expect(strings.scan.finished(6, 0)).toBe('Scan complete — 6 added.')
  expect(strings.scan.finished(3, 2)).toBe('Scan complete — 3 added, 2 already here.')
  expect(strings.upload.batchFinished(1, 1)).toBe('Upload complete — 1 added, 1 already here.')
})

test('a revised file is counted', () => {
  expect(strings.scan.finished(0, 4, 1)).toBe('Scan complete — 0 added, 1 revised, 4 already here.')
})
