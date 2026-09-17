import { useNavigate } from '@tanstack/react-router'
import { useEffect, useRef, useState, type RefObject } from 'react'
import { strings } from '../lib/strings'
import { Icon } from './Icon'

/**
 * The key that moves focus into search from anywhere on the grid, shown inside the field and
 * declared as its `aria-keyshortcuts`. `/` because the tools this audience already lives in
 * spend it on search, so it is a key people try before they read anything.
 *
 * A constant here and not a `strings.ts` entry: it is a key, not copy — nothing translates it,
 * and `KeyboardEvent.key` reports `/` on a Turkish layout too, where it sits on Shift+7.
 */
const SEARCH_SHORTCUT = '/'

/**
 * `/` from anywhere focuses `field`. Not while a person is typing into a field — a slash is
 * half of every file path — and not under a modal dialog, whose own keys own the page.
 */
function useSearchShortcut(field: RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== SEARCH_SHORTCUT || event.defaultPrevented) return
      if (event.ctrlKey || event.metaKey || event.altKey) return
      const target = event.target
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable || target.closest('input, textarea, select') !== null)
      ) {
        return
      }
      if (document.querySelector('[aria-modal="true"]') !== null) return
      event.preventDefault()
      field.current?.focus()
      field.current?.select()
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [field])
}

/**
 * The header's search on every page but the grid, where `SearchBox` is.
 *
 * It searches on Enter rather than as you type. `SearchBox` can debounce into the URL because
 * the grid it filters is on the same page; here the results are on another page, and
 * navigating away 250 ms into a pause would pull the field out from under the person typing.
 */
export function JumpSearch() {
  const navigate = useNavigate()
  const [typed, setTyped] = useState('')
  const field = useRef<HTMLInputElement>(null)
  useSearchShortcut(field)
  return (
    <form
      role="search"
      className="relative flex min-w-0 flex-1 items-center"
      onSubmit={(event) => {
        event.preventDefault()
        const query = typed.trim()
        void navigate({ to: '/', search: query.length < 2 ? {} : { q: query } })
      }}
    >
      <span className="pointer-events-none absolute left-[10px] text-[var(--color-muted)]">
        <Icon name="search" size={14} />
      </span>
      <input
        ref={field}
        type="search"
        value={typed}
        onChange={(event) => setTyped(event.target.value)}
        aria-label={strings.search.label}
        aria-keyshortcuts={SEARCH_SHORTCUT}
        placeholder={strings.search.placeholder}
        className="w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] py-2 pr-3 pl-[30px] text-[13px] focus:border-[var(--color-accent)]"
      />
    </form>
  )
}

/**
 * The search box, and the chip that says what it is searching.
 *
 * A `<input type="search">`, so the clear affordance, Escape-to-clear and the right mobile
 * keyboard come from the browser rather than from code here.
 *
 * **Local state, debounced navigation.** The field responds to every keystroke and the URL
 * does not: `navigate` on each one would render the route per character and — without
 * `replace` — put every character in the back button's history. 250 ms is the pause after
 * typing, not a delay before feedback.
 *
 * **Two characters minimum, and it is not arbitrary.** A trigram is three characters, so
 * under that `gin_trgm_ops` cannot be used at all and the query is a sequential scan by
 * construction. The box accepts the keystroke and says it is waiting.
 *
 * ponytail: two characters because of the index, not because of the product. If a
 * one-character search is ever wanted, the fix is a prefix index, not removing this.
 */
export function SearchBox({
  q,
  categoryName,
  filtered,
  onSearch,
  onWiden,
}: {
  q: string
  categoryName: string | null
  filtered: boolean
  onSearch?: (query: string) => void
  onWiden: () => void
}) {
  const [typed, setTyped] = useState(q)
  // The URL is the source of truth: a back navigation or a shared link has to move the box,
  // and without this the field would keep whatever was last typed into it.
  const [lastFromUrl, setLastFromUrl] = useState(q)
  if (q !== lastFromUrl) {
    setLastFromUrl(q)
    setTyped(q)
  }

  useEffect(() => {
    const trimmed = typed.trim()
    // Below the minimum the query is not run — but an empty box *is* a change, because it
    // means "show me the library again".
    if (trimmed.length === 1) return
    if (trimmed === q) return
    const timer = setTimeout(() => onSearch?.(trimmed), 250)
    return () => clearTimeout(timer)
  }, [typed, q, onSearch])

  const field = useRef<HTMLInputElement>(null)
  useSearchShortcut(field)

  const waiting = typed.trim().length === 1
  return (
    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-2">
      {/*
        The field sits *below* the ground rather than on it — `--color-raised` against the
        page, which is how `v2` draws every input. A control you type into reads as a well;
        one you press reads as a surface.
      */}
      <div className="relative flex min-w-0 flex-1 items-center">
        <span className="pointer-events-none absolute left-[10px] text-[var(--color-muted)]">
          <Icon name="search" size={14} />
        </span>
        <input
          ref={field}
          type="search"
          value={typed}
          onChange={(event) => setTyped(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== 'Escape') return
            // Back to the grid, keeping the query. Chrome's own Escape clears a search field,
            // which would throw away what was typed on the way out.
            event.preventDefault()
            document.getElementById('parts')?.focus()
          }}
          aria-label={strings.search.label}
          aria-keyshortcuts={SEARCH_SHORTCUT}
          placeholder={strings.search.placeholder}
          className="peer w-full rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] py-2 pr-9 pl-[30px] text-[13px] focus:border-[var(--color-accent)]"
        />
        {/*
          The key, shown where it is used. Only while the field is empty and unfocused: once
          you are in it the hint has done its job, and over typed text it would be noise.
        */}
        {typed === '' ? (
          <kbd
            aria-hidden="true"
            className="ease-mechanical pointer-events-none absolute right-2.5 rounded border border-[var(--color-edge)] px-1.5 font-mono text-[11px] leading-4 text-[var(--color-muted)] duration-[var(--duration-fast)] peer-focus:opacity-0"
          >
            {SEARCH_SHORTCUT}
          </kbd>
        ) : null}
      </div>
      {/*
        The disclosure, not a control that narrows. The sidebar has already narrowed the
        grid; a search that quietly kept that narrowing without saying so is how somebody
        concludes a part is missing from the library. Dismissing it widens and keeps the
        query.
      */}
      {!filtered || q === '' ? null : (
        <button
          type="button"
          onClick={onWiden}
          title={strings.search.widen}
          className="ease-mechanical flex items-center gap-1.5 rounded-full border border-[var(--color-accent)] bg-[var(--color-surface)] px-3 py-1 text-xs text-[var(--color-text)] duration-[var(--duration-fast)] hover:-translate-y-px"
        >
          {categoryName === null
            ? strings.search.inThisCategory
            : strings.search.inCategory(categoryName)}
          <Icon name="close" size={12} />
        </button>
      )}
      {!waiting ? null : (
        <p className="text-xs text-[var(--color-muted)]">{strings.search.keepTyping}</p>
      )}
    </div>
  )
}
