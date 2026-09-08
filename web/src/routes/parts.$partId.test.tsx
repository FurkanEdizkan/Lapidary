import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRouter,
} from '@tanstack/react-router'
import { beforeEach, expect, test, vi } from 'vitest'
import { PartPage } from './parts.$partId'
import { strings } from '../lib/strings'
import type { PartDetail } from '../lib/types'

/**
 * The part page, which is where `CLAUDE.md`'s measurement rules actually reach a screen.
 *
 * Most of these assert a refusal rather than a rendering: no volume for an open mesh, no
 * figure without its provenance, no "approximate" on an analytic value. Those are the
 * cases a page that simply printed its JSON would get wrong while looking entirely
 * correct, and none of them is visible from the API tests — the route hands back `null`
 * and it is this component that decides whether `null` reads as zero, as blank, or as a
 * sentence.
 */

const PART: PartDetail = {
  id: '01931b6e-0000-7000-8000-00000000aaaa',
  library: '01931b6e-0000-7000-8000-000000000001',
  revision: '01931b6e-0000-7000-8000-00000000bbbb',
  revLabel: '1',
  name: 'Bearing block, 608ZZ',
  partNumber: 'LP-1042-03',
  sourcePath: 'brackets/steel/LP-1042-03.stl',
  thumbnail: null,
  triangleCount: 48112,
  isWatertight: true,
  bboxMm: { value: [61, 42, 18.5], approximate: true },
  volumeMm3: { value: 21478.5, approximate: true },
  surfaceAreaMm2: { value: 9804.25, approximate: true },
  kernelVersion: 'mesh stl-1+cpu-1',
  sourceHash: '2222222222222222222222222222222222222222222222222222222222222222',
  sourceFormat: 'stl',
  sourceBytes: 204800,
  storedBytes: 91204,
  compressed: true,
  tessellationL0: '3333333333333333333333333333333333333333333333333333333333333333',
  tessellationL0Bytes: 7500,
  directory: 'libraries/default/Brackets/nema-17-motor-mount',
  storagePath: 'libraries/default/Brackets/nema-17-motor-mount/nema-17-motor-mount.stl',
  createdAt: '2026-09-06T10:00:00Z',
  updatedAt: '2026-09-06T10:00:00Z',
}

beforeEach(() => {
  vi.unstubAllGlobals()
})

/** A fetch that answers the detail route with `part`, or a status when given a number. */
/**
 * Answers the detail route with `part`, or with a status when given a number.
 *
 * URL-aware, and it has to be: the page fetches its gallery as well now, and a stub that
 * answered every request with a `PartDetail` handed `[].map` an object. That did not fail
 * these tests — the gallery query had not resolved by the time they asserted — which is the
 * kind of latent flake worth closing at the stub rather than discovering later.
 */
function stub(
  part: PartDetail | number,
  images: unknown[] = [],
  /** Which of the page's two secondary fetches should fail, for the tests that need one to. */
  broken: { images?: boolean; sources?: boolean } = {},
) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      if (url.endsWith('/images')) {
        if (broken.images) return { ok: false, status: 503, json: async () => ({}) }
        return { ok: true, status: 200, json: async () => images }
      }
      // The page reads its sources too. Same reason as the gallery above: a stub that
      // answered this with a `PartDetail` would hand `[].map` an object.
      if (url.endsWith('/sources')) {
        if (broken.sources) return { ok: false, status: 503, json: async () => ({}) }
        return { ok: true, status: 200, json: async () => [] }
      }
      return typeof part === 'number'
        ? { ok: false, status: part, json: async () => ({}) }
        : { ok: true, status: 200, json: async () => part }
    }),
  )
}

/** The page needs a router in scope: it renders a `<Link to="/">` back to the grid. */
function renderPage() {
  const rootRoute = createRootRoute({ component: () => <PartPage partId={PART.id} /> })
  const router = createRouter({
    routeTree: rootRoute,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RouterProvider router={router as never} />
    </QueryClientProvider>,
  )
}

/** The row whose `<dt>` is `label`, so an assertion names the figure it means. */
async function row(label: string): Promise<HTMLElement> {
  const term = await screen.findByText(label)
  const value = term.nextElementSibling
  expect(value).not.toBeNull()
  return value as HTMLElement
}

test('a tessellated figure is labelled and an analytic one is not', async () => {
  // The Phase 2 shape, and the reason the badge is per figure rather than per page: one
  // revision carrying an analytic volume beside a tessellated surface area. A page-level
  // badge is wrong about one of them whichever way it is set.
  stub({
    ...PART,
    volumeMm3: { value: 21478.5, approximate: false },
    surfaceAreaMm2: { value: 9804.25, approximate: true },
  })
  renderPage()

  const volume = await row(strings.detail.volume)
  expect(within(volume).queryByText(strings.detail.approximate)).toBeNull()
  const area = await row(strings.detail.surfaceArea)
  expect(within(area).getByText(strings.detail.approximate)).toBeDefined()
})

test('an open mesh says why it has no volume rather than showing a blank', async () => {
  // "Measurement must not lie" includes declining to measure. A blank here reads as zero,
  // and zero is a number this part does not have.
  stub({ ...PART, isWatertight: false, volumeMm3: null })
  renderPage()

  const volume = await row(strings.detail.volume)
  expect(within(volume).getByText(strings.detail.volumeUnavailable)).toBeDefined()
})

test('a missing volume on a closed mesh is unknown, not a refusal to measure', async () => {
  // The other `null`. "Not available — the mesh is not closed" would be a false
  // explanation for a watertight part whose figure simply was not recorded.
  stub({ ...PART, isWatertight: true, volumeMm3: null })
  renderPage()

  const volume = await row(strings.detail.volume)
  expect(within(volume).getByText(strings.detail.unknown)).toBeDefined()
  expect(within(volume).queryByText(strings.detail.volumeUnavailable)).toBeNull()
})

test('the L0 rung is offered by hash, which is what makes those bytes reachable', async () => {
  // Until this link existed, every ingest wrote a rung that nothing could address:
  // `GET /api/blob/{blake3}` had no possible caller.
  stub(PART)
  renderPage()

  const preview = await row(strings.detail.preview3d)
  const link = within(preview).getByRole('link')
  expect(link.getAttribute('href')).toBe(`/api/blob/${PART.tessellationL0}`)
})

test('a part with no rung says so instead of linking to bytes that do not exist', async () => {
  stub({ ...PART, tessellationL0: null, tessellationL0Bytes: null })
  renderPage()

  const preview = await row(strings.detail.preview3d)
  expect(within(preview).getByText(strings.detail.noPreview3d)).toBeDefined()
  expect(within(preview).queryByRole('link')).toBeNull()
})

test('the path is shown, because it is what tells two parts of the same name apart', async () => {
  // Slice 6a moved identity from the name to the path. Two parts called `bracket` in two
  // folders are one part and one silent skip without it, so a page that omitted it could
  // not answer "which bracket is this".
  stub(PART)
  renderPage()

  const path = await row(strings.detail.sourcePath)
  expect(within(path).getByText('brackets/steel/LP-1042-03.stl')).toBeDefined()
})

test('a part that is gone shows one actionable message', async () => {
  // A 404 and an unreachable api land here alike: this page was reached from a grid that
  // may be stale, and going back is what refreshes it either way.
  stub(404)
  renderPage()

  expect(await screen.findByText(strings.detail.failed)).toBeDefined()
})

/**
 * A photograph appears **above** the render rather than instead of it. The generated view is
 * the honest picture of the geometry and stays; the photograph is what the geometry cannot
 * show. `part_image`'s ordering column exists so both can, which was an owner decision.
 */
test('shows the pictures attached to a part, alongside the render', async () => {
  stub(PART, [
    {
      id: '01a07c10-0000-7000-8000-000000000001',
      src: 'data:image/webp;base64,UklGRg==',
      origin: 'uploaded',
      sourceUrl: null,
      fit: 'cover',
      focusX: 0.5,
      focusY: 0.5,
    },
  ])
  renderPage()

  const picture = await screen.findByAltText(strings.images.alt(PART.name, 0))
  expect(picture.getAttribute('src')).toBe('data:image/webp;base64,UklGRg==')
})

/**
 * A refused file keeps the server's own sentence, which names the limit that was broken.
 * A generic "upload failed" would drop the one thing the person who picked the file needs.
 */
test('a refused picture shows the reason the server gave', async () => {
  stub(PART)
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string }) => {
      if (url.endsWith('/images') && init?.method === 'POST') {
        return {
          ok: false,
          status: 422,
          json: async () => ({
            message: 'That image is 16×16, and the smallest side must be at least 64 pixels.',
          }),
        }
      }
      if (url.endsWith('/images')) return { ok: true, status: 200, json: async () => [] }
      if (url.endsWith('/sources')) return { ok: true, status: 200, json: async () => [] }
      return { ok: true, status: 200, json: async () => PART }
    }),
  )
  renderPage()

  const picker = await screen.findByRole('button', { name: strings.images.add })
  const input = picker.previousElementSibling as HTMLInputElement
  fireEvent.change(input, {
    target: { files: [new File(['not really a png'], 'icon.png', { type: 'image/png' })] },
  })

  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    'That image is 16×16, and the smallest side must be at least 64 pixels.',
  )
})

/**
 * The other way in, end to end through the client: the address goes to `/from-url` as JSON
 * and the gallery is re-read. What this pins down is the *route* — sending the URL to the
 * upload route as a raw body would reach a server that tried to decode it as an image and
 * refused it with a sentence about file formats, which is a confusing way to be told the
 * client posted to the wrong place.
 */
test('pasting an address posts it to the fetch route and shows the picture', async () => {
  stub(PART)
  let posted: { url: string } | null = null
  let gallery: unknown[] = []
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (url.endsWith('/images/from-url')) {
        posted = JSON.parse(init?.body ?? '{}') as { url: string }
        gallery = [
          {
            id: '01931b6e-0000-7000-8000-0000000000ff',
            src: 'data:image/webp;base64,UklGRg==',
            origin: 'url_supplied',
            sourceUrl: posted.url,
            fit: 'cover',
            focusX: 0.5,
            focusY: 0.5,
          },
        ]
        return { ok: true, status: 201, json: async () => ({ id: 'x', width: 900, height: 600 }) }
      }
      if (url.endsWith('/images')) return { ok: true, status: 200, json: async () => gallery }
      if (url.endsWith('/sources')) return { ok: true, status: 200, json: async () => [] }
      return { ok: true, status: 200, json: async () => PART }
    }),
  )
  renderPage()

  fireEvent.click(await screen.findByRole('button', { name: strings.images.addFromUrl }))
  fireEvent.change(screen.getByRole('textbox', { name: strings.images.urlLabel }), {
    target: { value: 'https://example.com/idler-pulley.jpg' },
  })
  fireEvent.click(screen.getByRole('button', { name: strings.images.fetch }))

  const picture = await screen.findByAltText(strings.images.alt(PART.name, 0))
  expect(picture.getAttribute('title')).toBe(
    strings.images.from('https://example.com/idler-pulley.jpg'),
  )
  expect(posted).toEqual({ url: 'https://example.com/idler-pulley.jpg' })
})

/**
 * **The refusal a person will actually hit**, and the reason `uploadPartImage` treats these
 * statuses as answers rather than failures: the server's sentence explains that the address
 * points inside a private network, which "the picture could not be stored" could not.
 */
test('an address the server will not fetch from keeps the explanation it gave', async () => {
  stub(PART)
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      if (url.endsWith('/images/from-url')) {
        return {
          ok: false,
          status: 422,
          json: async () => ({
            message: 'That address is not one this server will fetch from.',
          }),
        }
      }
      if (url.endsWith('/images')) return { ok: true, status: 200, json: async () => [] }
      if (url.endsWith('/sources')) return { ok: true, status: 200, json: async () => [] }
      return { ok: true, status: 200, json: async () => PART }
    }),
  )
  renderPage()

  fireEvent.click(await screen.findByRole('button', { name: strings.images.addFromUrl }))
  fireEvent.change(screen.getByRole('textbox', { name: strings.images.urlLabel }), {
    target: { value: 'http://169.254.169.254/latest/meta-data/' },
  })
  fireEvent.click(screen.getByRole('button', { name: strings.images.fetch }))

  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    'That address is not one this server will fetch from.',
  )
})

/**
 * **The framing is CSS, and this is the assertion that says so.** A version that cropped by
 * re-encoding, or by computing a transform in JavaScript, would fail here — the `src` is
 * untouched and the framing is two style properties the browser applies.
 */
test('a picture is framed by its row rather than by its bytes', async () => {
  stub(PART, [
    {
      id: '01931b6e-0000-7000-8000-0000000000aa',
      src: 'data:image/webp;base64,UklGRg==',
      origin: 'uploaded',
      sourceUrl: null,
      fit: 'contain',
      focusX: 0.25,
      focusY: 0.75,
    },
  ])
  renderPage()

  const picture = await screen.findByAltText(strings.images.alt(PART.name, 0))
  expect(picture.style.objectFit).toBe('contain')
  expect(picture.style.objectPosition).toBe('25% 75%')
})

/** Clicking the picture chooses what stays in frame, and sends it as fractions of an edge. */
test('clicking a picture sets its focal point', async () => {
  const image = {
    id: '01931b6e-0000-7000-8000-0000000000aa',
    src: 'data:image/webp;base64,UklGRg==',
    origin: 'uploaded' as const,
    sourceUrl: null,
    fit: 'cover' as const,
    focusX: 0.5,
    focusY: 0.5,
  }
  let patched: { fit: string; focusX: number; focusY: number } | null = null
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (init?.method === 'PATCH') {
        patched = JSON.parse(init.body ?? '{}') as typeof patched
        return { ok: true, status: 204, json: async () => ({}) }
      }
      if (url.endsWith('/images')) return { ok: true, status: 200, json: async () => [image] }
      if (url.endsWith('/sources')) return { ok: true, status: 200, json: async () => [] }
      return { ok: true, status: 200, json: async () => PART }
    }),
  )
  renderPage()

  const control = await screen.findByRole('button', {
    name: strings.images.focusLabel(strings.images.alt(PART.name, 0)),
  })
  // jsdom gives every element a zero-sized box, so a click's coordinates cannot be turned
  // into a fraction. The keyboard path is the same code with a known step, and it is the
  // path somebody without a pointer uses anyway.
  fireEvent.keyDown(control, { key: 'ArrowRight' })

  await waitFor(() => expect(patched).not.toBeNull())
  expect(patched).toEqual({ fit: 'cover', focusX: 0.6, focusY: 0.5 })
})

/**
 * A recorded source, shown whole — the licence especially, which is the field this section
 * exists for: somebody selling prints has to see a model was non-commercial before printing.
 */
test('a recorded source shows its licence and its price', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) => {
      if (url.endsWith('/images')) return { ok: true, status: 200, json: async () => [] }
      if (url.endsWith('/sources')) {
        return {
          ok: true,
          status: 200,
          json: async () => [
            {
              id: '01931b6e-0000-7000-8000-0000000000bb',
              url: 'https://www.printables.com/model/482910',
              vendor: 'Printables',
              externalId: '482910',
              title: 'Idler pulley, 20 tooth',
              license: 'CC-BY-NC-SA 4.0',
              priceMinor: 0,
              currency: 'USD',
            },
          ],
        }
      }
      return { ok: true, status: 200, json: async () => PART }
    }),
  )
  renderPage()

  const link = await screen.findByRole('link', { name: 'Idler pulley, 20 tooth' })
  expect(link.getAttribute('href')).toBe('https://www.printables.com/model/482910')
  // `noreferrer` as well as `noopener`: this address came from somewhere else, and where a
  // private parts library lives is not something to hand back to it.
  expect(link.getAttribute('rel')).toBe('noopener noreferrer')
  expect(await screen.findByText(/CC-BY-NC-SA 4\.0/)).toBeTruthy()
})

/**
 * The price the form sends. A decimal is how a price is written and minor units are how it
 * is stored, and `12.34 * 100` is `1233.9999999999998` — a price that rounds down by a penny
 * on the way in is a bug nobody would ever find by looking at it.
 */
test('a price typed as a decimal is sent as exact minor units', async () => {
  let posted: { priceMinor: number | null; license: string | null } | null = null
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: { method?: string; body?: string }) => {
      if (url.endsWith('/sources') && init?.method === 'POST') {
        posted = JSON.parse(init.body ?? '{}') as typeof posted
        return { ok: true, status: 201, json: async () => ({}) }
      }
      if (url.endsWith('/images')) return { ok: true, status: 200, json: async () => [] }
      if (url.endsWith('/sources')) return { ok: true, status: 200, json: async () => [] }
      return { ok: true, status: 200, json: async () => PART }
    }),
  )
  renderPage()

  fireEvent.click(await screen.findByRole('button', { name: strings.sources.add }))
  fireEvent.change(screen.getByLabelText(strings.sources.priceField), {
    target: { value: '12.34' },
  })
  fireEvent.change(screen.getByLabelText(strings.sources.license), {
    target: { value: 'CC-BY 4.0' },
  })
  fireEvent.click(screen.getByRole('button', { name: strings.sources.save }))

  // Asserted on the object rather than through `posted?.priceMinor`: the variable is only
  // ever assigned inside the stub's closure, so TypeScript's flow analysis has it as `null`
  // at this point and an optional chain off it narrows to `never`.
  await waitFor(() => expect(posted).not.toBeNull())
  expect(posted).toMatchObject({ priceMinor: 1234, license: 'CC-BY 4.0' })
})

/**
 * **Every per-part function is reachable without a mouse, and this is the test that says so.**
 *
 * WCAG 2.2 SC 2.1.1 is Level A and it is about whether a *function* is available from a
 * keyboard at all — not about which surface offers it. For a while three were not: Render
 * preview, Move to… and Show storage path lived only in the grid's quick-look panel, and the
 * panel opens on a click of a tile that carries no `role`, no `tabIndex` and no key handler.
 * The card's name is a real link and it comes here, so here is where they have to be.
 *
 * Asserted by role and accessible name rather than by class, so a restyle cannot break it and
 * a control that stops being a button will.
 */
test('every per-part action is reachable on the page a keyboard can get to', async () => {
  stub(PART)
  renderPage()

  // The three that were mouse-only, plus the two that were always here.
  for (const name of [
    strings.render.part,
    strings.folders.moveTo,
    strings.folders.showInFolderFor(PART.name),
    strings.removal.remove,
  ]) {
    expect(await screen.findByRole('button', { name })).toBeTruthy()
  }
  expect(screen.getByRole('link', { name: strings.download.original })).toBeTruthy()

  // And none of them is hidden from the accessibility tree behind an inert wrapper: a
  // control that exists but is `aria-hidden` or `display:none` satisfies a query that asks
  // only for presence, which is the assertion this repository keeps catching.
  for (const name of [strings.render.part, strings.folders.moveTo]) {
    expect(screen.getByRole('button', { name, hidden: false })).toBeTruthy()
  }
})

/** The storage path is a disclosure, so the button has to actually reveal something. */
test('show storage path reveals the part path on the detail page', async () => {
  stub(PART)
  renderPage()

  fireEvent.click(
    await screen.findByRole('button', { name: strings.folders.showInFolderFor(PART.name) }),
  )
  expect(await screen.findByText(PART.storagePath as string)).toBeTruthy()
})


/**
 * SC 2.4.2, Level A. `index.html` ships one static `<title>` for the whole application,
 * and `document.title` reads the *first* title in tree order — so this seeds that tag
 * before rendering. Without it the assertion passes in an empty jsdom head whether the
 * route sets a title or not, which is a check that cannot fail.
 */
test('the tab carries the part name, not the application name', async () => {
  document.head.innerHTML = '<title>Lapidary</title>'
  stub(PART)
  renderPage()

  await waitFor(() => expect(document.title).toBe('Bearing block, 608ZZ — Lapidary'))
})

/** A part page that has not resolved yet must not name a part it does not have. */
test('the tab does not guess a name before the page has one', async () => {
  document.head.innerHTML = '<title>Lapidary</title>'
  stub(404)
  renderPage()

  await screen.findByText(strings.detail.failed)
  expect(document.title).toBe('Part — Lapidary')
})

/**
 * `sources.data ?? []` reads a failed fetch as a part with nothing recorded. The licence
 * is the field this section exists for — `docs/DATA.md` is emphatic that somebody selling
 * prints needs to see a non-commercial licence before they print — so "we could not load
 * it" and "there is none" have to be different sentences.
 */
test('sources that could not be loaded do not read as sources that do not exist', async () => {
  stub(PART, [], { sources: true })
  renderPage()

  await screen.findByText(strings.sources.failed)
})

test('a gallery that could not be loaded does not read as a part with no pictures', async () => {
  stub(PART, [], { images: true })
  renderPage()

  await screen.findByText(strings.images.galleryFailed)
})
