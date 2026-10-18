import { act, fireEvent, render, screen, within } from '@testing-library/react'
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
    <AppFrame current="dashboard">
      <p>{strings.dashboard.title}</p>
    </AppFrame>
  ), '/dashboard')
  const nav = await screen.findByRole('navigation', { name: strings.frame.places })
  const marked = [...nav.querySelectorAll('[aria-current="page"]')].map((place) => place.textContent)
  expect(marked).toEqual([strings.dashboard.title])
  expect(screen.getByRole('link', { name: strings.frame.parts })).toBeDefined()
})

/**
 * M2: the places are the grid and the dashboard. Profile and Settings are the user menu's, at the
 * header's right.
 */
test('Profile and Settings are in the user menu, not among the places', async () => {
  renderInRouter(() => (
    <AppFrame current="profile">
      <p>{strings.profile.lead}</p>
    </AppFrame>
  ), '/profile')
  const nav = await screen.findByRole('navigation', { name: strings.frame.places })
  expect(nav.textContent).not.toContain(strings.settings.title)
  expect(screen.getByRole('button', { name: strings.user.menu })).toBeDefined()
  // A closed popover is out of jsdom's accessibility tree; it is found by the id its button names.
  const menu = document.getElementById('user-menu') as HTMLElement
  expect(menu.querySelector('[aria-current="page"]')?.textContent).toBe(strings.user.profile)
  expect([...menu.querySelectorAll('a')].map((a) => [a.textContent, a.getAttribute('href')])).toEqual([
    [strings.settings.title, '/settings'],
  ])
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

/**
 * A21: a current place on every page. A page inside Parts (a part's page, the tag index) marks Parts and keeps it a
 * link back to the grid; the account's pages mark the account button. One marker, always, because it travels by one id.
 */
test.each([
  ['part', strings.frame.parts],
  ['tags', strings.frame.parts],
  ['sharedWithMe', strings.sharing.place],
] as const)('on %s the marker is under %s', async (current, place) => {
  renderInRouter(() => (
    <AppFrame current={current}>
      <p>{strings.parts.loading}</p>
    </AppFrame>
  ), '/elsewhere')
  const nav = await screen.findByRole('navigation', { name: strings.frame.places })
  const markers = document.querySelectorAll('[data-place-marker]')
  expect(markers).toHaveLength(1)
  expect(markers[0]?.parentElement?.textContent).toBe(place)
  if (current !== 'sharedWithMe') {
    expect(within(nav).getByRole('link', { name: strings.frame.parts })).toBeDefined()
    expect(nav.querySelector('[aria-current="page"]')).toBeNull()
  }
})

test.each(['siteSettings', 'profile', 'people'] as const)('on %s the account button is marked', async (current) => {
  renderInRouter(() => (
    <AppFrame current={current}>
      <p>{strings.parts.loading}</p>
    </AppFrame>
  ), '/elsewhere')
  const account = await screen.findByRole('button', { name: strings.user.menu })
  const markers = document.querySelectorAll('[data-place-marker]')
  expect(markers).toHaveLength(1)
  expect(markers[0]?.parentElement?.contains(account)).toBe(true)
})

/** One row under `md`: the page's tools live in the header until the drawer opens, then only in the drawer. */
test('the tools move into the drawer while it is open, and are never in the page twice', async () => {
  renderInRouter(() => (
    <AppFrame current="parts" tools={<button type="button">{strings.addModels.button}</button>}>
      <p>{strings.parts.loading}</p>
    </AppFrame>
  ))
  const header = await screen.findByRole('banner')
  expect(within(header).getAllByRole('button', { name: strings.addModels.button })).toHaveLength(1)
  fireEvent.click(screen.getByRole('button', { name: strings.frame.openDrawer }))
  expect(screen.getAllByRole('button', { name: strings.addModels.button })).toHaveLength(1)
  expect(document.getElementById('drawer')?.textContent).toContain(strings.addModels.button)
})

/** The search folds to an icon under `md` and stays in the document, so `/` and the rig still find the field. */
test('the search button opens the search across the row and focuses it', async () => {
  renderInRouter(() => (
    <AppFrame current="dashboard">
      <p>{strings.parts.loading}</p>
    </AppFrame>
  ))
  const open = await screen.findByRole('button', { name: strings.frame.openSearch })
  expect(screen.getByRole('searchbox', { name: strings.search.label })).toBeDefined()
  fireEvent.click(open)
  expect(open.getAttribute('aria-expanded')).toBe('true')
  expect(document.activeElement).toBe(screen.getByRole('searchbox', { name: strings.search.label }))
  fireEvent.keyDown(document.activeElement as Element, { key: 'Escape' })
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
  const { rerender, container } = render(<UploadButton onFiles={() => {}} onFolder={() => {}} busy={false} progress={undefined} />)
  expect(container.innerHTML).not.toContain('--color-accent')

  rerender(
    <UploadButton
      onFiles={() => {}}
      onFolder={() => {}}
      busy
      progress={{ phase: 'transferring', filesDone: 3, filesTotal: 12, bytesSent: 25, bytesToSend: 100, alreadyHere: 0 }}
    />,
  )
  expect(container.innerHTML).toContain('--color-accent')
  expect(container.textContent).toContain('25%')
})

/** A10: Upload opens a menu, and its two items open the files picker and the folder picker. */
test('Upload offers files, archives among them, or a folder', () => {
  const onFiles = vi.fn()
  const onFolder = vi.fn()
  render(<UploadButton onFiles={onFiles} onFolder={onFolder} busy={false} progress={undefined} />)
  fireEvent.click(screen.getByText(strings.toolbar.uploadFiles))
  expect(onFiles).toHaveBeenCalledTimes(1)
  fireEvent.click(screen.getByText(strings.toolbar.uploadAFolder))
  expect(onFolder).toHaveBeenCalledTimes(1)
  expect(screen.getByText(strings.toolbar.uploadFilesHint)).toBeDefined()
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

/** The mark goes home, as a logo does (M1: it did not, and there was no other way back but Parts). */
test('the Lapidary mark is a link to the grid', async () => {
  renderInRouter(() => (
    <AppFrame current="dashboard">
      <p>{strings.dashboard.title}</p>
    </AppFrame>
  ), '/dashboard')
  const home = await screen.findByRole('link', { name: strings.appName })
  expect(home.getAttribute('href')).toBe('/')
})
