import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'
import { Dialog } from './Dialog'
import { strings } from '../lib/strings'

afterEach(cleanup)

/**
 * Where focus lands when a dialog opens. The box used to take it itself, which put the
 * focus ring around the whole panel — a ring that says "everything is selected" and points
 * at nothing a key can operate.
 */
test('a dialog with nothing to fill in opens with focus on its close button, not on the box', () => {
  render(
    <Dialog title="Move vee-block-lp-3072-02" onClose={vi.fn()}>
      <p>Choose the category it moves into.</p>
    </Dialog>,
  )

  expect(document.activeElement).toBe(screen.getByRole('button', { name: strings.dialog.close }))
})

test('a field that asks for focus keeps it', () => {
  render(
    <Dialog title="New category" onClose={vi.fn()}>
      <input aria-label="Category name" autoFocus />
    </Dialog>,
  )

  expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Category name' }))
})
