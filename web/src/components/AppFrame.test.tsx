import { act, fireEvent, render, screen } from '@testing-library/react'
import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from '@tanstack/react-router'
import { useRef } from 'react'
import { expect, test, vi } from 'vitest'
import { AppFrame } from './AppFrame'
import { DropOverlay, UploadButton } from './Upload'
import { strings } from '../lib/strings'

vi.mock('../lib/upload', () => ({
  filesFromDrop: vi.fn(async () => [{ file: new File(['solid'], 'spur-gear-m1-z20-lp-2020-01.stl'), path: 'gears/spur-gear-m1-z20-lp-2020-01.stl' }]),
  filesFromInput: vi.fn(() => []),
}))

/** Under a router, because the places are links. */
function renderInRouter(ui: () => React.ReactNode, at = '/') {
  const router = createRouter({
    routeTree: createRootRoute({ component: ui }),
    history: createMemoryHistory({ initialEntries: [at] }),
  })
  return render(<RouterProvider router={router as never} />)
}

test('the page you are on is marked, and the other places are links', async () => {
  renderInRouter(() => (
    <AppFrame current="removed">
      <p>{strings.removal.removedLead}</p>
    </AppFrame>
  ), '/removed')
  const nav = await screen.findByRole('navigation', { name: strings.frame.places })
  const marked = [...nav.querySelectorAll('[aria-current="page"]')].map((place) => place.textContent)
  expect(marked).toEqual([strings.removal.removedTitle])
  expect(screen.getByRole('link', { name: strings.frame.parts })).toBeDefined()
  expect(screen.getByRole('link', { name: strings.sharing.title })).toBeDefined()
  expect(screen.queryByRole('link', { name: strings.removal.removedTitle })).toBeNull()
})

/**
 * The narrow-screen drawer. jsdom applies no stylesheet, so what is checked is the state the
 * classes hang on — `aria-expanded` — and where focus goes, which is the part a keyboard feels.
 */
test('the drawer opens, and Escape closes it and returns to its button', async () => {
  renderInRouter(() => (
    <AppFrame current="parts" rail={<p>{strings.folders.root}</p>}>
      <p>{strings.parts.loading}</p>
    </AppFrame>
  ))
  const open = await screen.findByRole('button', { name: strings.frame.openDrawer })
  expect(open.getAttribute('aria-expanded')).toBe('false')

  // One nav while closed; the drawer's copy of the places exists only while it is open.
  expect(screen.getAllByRole('navigation')).toHaveLength(1)
  fireEvent.click(open)
  expect(open.getAttribute('aria-expanded')).toBe('true')
  expect(document.activeElement).toBe(screen.getByRole('button', { name: strings.frame.closeDrawer }))

  fireEvent.keyDown(document, { key: 'Escape' })
  expect(open.getAttribute('aria-expanded')).toBe('false')
  expect(document.activeElement).toBe(open)
})

test('the crash frame has the mark and nothing that needs a router', () => {
  render(
    <AppFrame nav={false} search={null}>
      <p>{strings.crash.body}</p>
    </AppFrame>,
  )
  expect(screen.getByRole('heading', { name: strings.appName })).toBeDefined()
  expect(screen.queryByRole('navigation')).toBeNull()
  expect(screen.queryByRole('search')).toBeNull()
})

/** DESIGN.md: Layout Blue marks what is live. An Upload button waiting for a click is not. */
test('Upload wears no accent at rest, and an accent bar while an upload runs', () => {
  const { rerender, container } = render(<UploadButton onUpload={() => {}} busy={false} progress={undefined} />)
  expect(container.innerHTML).not.toContain('--color-accent')

  rerender(
    <UploadButton
      onUpload={() => {}}
      busy
      progress={{ phase: 'transferring', filesDone: 3, filesTotal: 12, bytesSent: 25, bytesToSend: 100, alreadyHere: 0, bytesSkipped: 0 }}
    />,
  )
  expect(container.innerHTML).toContain('--color-accent')
  expect(container.textContent).toContain('25%')
})

function Overlay({ onFiles }: { onFiles: () => void }) {
  const picker = useRef<HTMLInputElement>(null)
  return <DropOverlay onFiles={onFiles} picker={picker} />
}

const overlay = () => document.querySelector('[data-drop-overlay]')

test('a drag carrying files raises the drop overlay, and dropping hands the files on', async () => {
  const onFiles = vi.fn()
  render(<Overlay onFiles={onFiles} />)
  expect(overlay()).toBeNull()

  fireEvent.dragEnter(window, { dataTransfer: { types: ['Files'] } })
  expect(overlay()).not.toBeNull()
  expect(screen.getByText(strings.upload.dropNow)).toBeDefined()

  await act(async () => {
    fireEvent.drop(window, { dataTransfer: { types: ['Files'], items: [] } })
  })
  expect(overlay()).toBeNull()
  expect(onFiles).toHaveBeenCalledTimes(1)
})

test('a drag that leaves the window takes the overlay with it', () => {
  render(<Overlay onFiles={() => {}} />)
  fireEvent.dragEnter(window, { dataTransfer: { types: ['Files'] } })
  fireEvent.dragEnter(window, { dataTransfer: { types: ['Files'] } })
  fireEvent.dragLeave(window, { dataTransfer: { types: ['Files'] } })
  expect(overlay()).not.toBeNull()
  fireEvent.dragLeave(window, { dataTransfer: { types: ['Files'] } })
  expect(overlay()).toBeNull()
})

/** A card dragged onto a category carries the part, not files, and must not look like an upload. */
test('dragging a part does not raise the drop overlay', () => {
  render(<Overlay onFiles={() => {}} />)
  fireEvent.dragEnter(window, { dataTransfer: { types: ['application/x-lapidary-part'] } })
  expect(overlay()).toBeNull()
})
