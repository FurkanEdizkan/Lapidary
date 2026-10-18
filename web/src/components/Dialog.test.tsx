import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, expect, test, vi } from 'vitest'
import { Dialog } from './Dialog'
import { MotionRoot, Presence } from '../lib/motion-react'
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

/**
 * Leaving inside `Presence` (A22): the box stays in `<body>` for its exit, and in that time it is not a dialog to
 * anybody — the rig's `[role="dialog"]` queries included — and focus is already back on what opened it.
 */
test('a dialog leaving through Presence stops being a dialog at once and gives focus back', async () => {
  function Host({ open }: { open: boolean }) {
    return (
      <MotionRoot>
        <button type="button">Share</button>
        <Presence>
          {open ? (
            <Dialog key="share" title="Share 2 models" onClose={vi.fn()}>
              <p>Who sees these.</p>
            </Dialog>
          ) : null}
        </Presence>
      </MotionRoot>
    )
  }
  const { rerender } = render(<Host open={false} />)
  const trigger = screen.getByRole('button', { name: 'Share' })
  trigger.focus()
  rerender(<Host open />)
  expect(screen.getByRole('dialog', { name: 'Share 2 models' })).not.toBeNull()
  // Motion's features arrive lazily; until they have, nothing animates and a leaving box goes at once.
  await act(async () => {
    await import('../lib/motion-features')
    await new Promise((resolve) => setTimeout(resolve, 0))
  })

  rerender(<Host open={false} />)
  expect(document.querySelector('[role="dialog"]')).toBeNull()
  expect(document.activeElement).toBe(trigger)
  // Still in the document, leaving: the exit is real, so the checks above were made against a box on its way out.
  expect(screen.getByText('Who sees these.')).not.toBeNull()
})
