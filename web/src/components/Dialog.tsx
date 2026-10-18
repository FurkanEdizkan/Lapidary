import { useEffect, useId, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { Fade, Panel, usePresent } from '../lib/motion-react'
import { strings } from '../lib/strings'
import { Icon } from './Icon'

/** Everything inside the box that a Tab can land on. `:not([disabled])` is the point. */
const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

/**
 * Close the toolbar menu `control` sits in, for a control that opens a dialog. A menu is a native popover, which a
 * click inside it never dismisses, and a dialog is drawn outside it, so the menu would stay open beside the dialog.
 */
export function closeMenu(control: Element) {
  control.closest<HTMLElement>('[data-menu]')?.hidePopover?.()
}

/** A dialog's foot: right-aligned, the quiet actions first and the one standing verb (`Page.ts`'s `STANDING`) last. */
export const DIALOG_FOOT = 'mt-6 flex flex-wrap items-center justify-end gap-2'

/**
 * Every action in a dialog that is not its verb, and the verb itself until it can act: bounded by Edge (The Operable
 * Edge Rule), Graphite lifting to Chalk, the same height as `STANDING` so a foot lines up.
 */
export const DIALOG_QUIET =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-2 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-50'

/** A section inside a dialog: a Label over a scribe line, tighter than a page's (12–16px inside a dialog). */
export const DIALOG_SECTION = 'mt-5 border-t border-[var(--color-border)] pt-3'

/**
 * The shell every dialog here shares: `role="dialog"` + `aria-modal`, Escape, a focus trap
 * and a portal. The caller autofocuses whichever control the safe answer is — cancel where
 * the action is destructive, confirm where it is not.
 *
 * **Portaled to `<body>`, and that is a layout fix before it is an accessibility one.** A
 * card is `overflow-hidden hover:-translate-y-0.5`; Tailwind 4 emits `-translate-y-*` as
 * the `translate` property, and a `translate` other than `none` makes an element a
 * containing block for fixed-position descendants (CSS Transforms 2 §3, which names
 * `translate` alongside `transform`). Rendered inside the card, this overlay's `fixed
 * inset-0` therefore resolved against the card's padding box and was clipped to it for as
 * long as the pointer stayed on the card — a squashed panel inside an 11rem card that
 * snapped to a full-viewport modal when the mouse left. The keyboard path never showed it,
 * because a keyboard never hovers. jsdom computes no layout, so no test here can see it
 * either; the fix is structural, and the portal is what makes it structural.
 *
 * **Escape is listened for on the document, not on the overlay.** Pressing an action
 * disables the button that had focus, a disabled button loses it, and focus falls to
 * `<body>` — which is not a descendant of the overlay, so an `onKeyDown` there stops
 * receiving keys and the dialog becomes keyboard-undismissable exactly when a request is
 * in flight or has just been refused. The document hears the key wherever focus went.
 *
 * **The trap is Tab-shaped rather than `inert`-shaped** for the same reason: `inert` on
 * the background needs a wrapper this component does not own, while wrapping Tab at the
 * two ends of the box — and pulling focus back in when it is nowhere — is the whole of
 * what `aria-modal="true"` is currently asserting and nothing was enforcing.
 *
 * **Arriving and leaving (A22).** The scrim fades and the box rises 8px from 0.98 over `--duration-base`. To leave
 * as well, the caller mounts it inside `Presence` (`<Presence>{open ? <Dialog …/> : null}</Presence>`); without one
 * it simply vanishes, as before. While it leaves it is already gone in every way but the picture: hidden from
 * assistive tech, `inert`, the scrim lets clicks through, Escape and the trap are off and focus has been given back,
 * so the 120 ms exit never holds anybody up.
 */
export function Dialog({
  title,
  onClose,
  children,
  size = 'default',
  returnFocus,
}: {
  title: string
  onClose: () => void
  children: ReactNode
  /** `stage` is the quick look's: most of the window, for a part in 3D. `wide` is the folder picker's. */
  size?: 'default' | 'wide' | 'stage'
  /**
   * Where focus goes on close, when that is not what opened the dialog: the quick look steps from
   * part to part, and closing it returns to the card now on screen rather than the first one.
   */
  returnFocus?: () => HTMLElement | null
}) {
  const titleId = useId()
  const box = useRef<HTMLDivElement>(null)
  const present = usePresent()
  // Read by `onBlur`, which can fire in the commit that makes the box inert, before its new props are in place.
  const leaving = useRef(false)
  leaving.current = !present
  // Every caller passes an inline arrow, so `onClose` is a new function each render. Read
  // through a ref rather than depending on it: an effect keyed on the callback would tear
  // down and re-run on every render, and its cleanup would throw focus back at the trigger
  // while the dialog was still open.
  const close = useRef(onClose)
  const giveBack = useRef(returnFocus)
  useEffect(() => {
    close.current = onClose
    giveBack.current = returnFocus
  })

  /**
   * Whatever had focus when this dialog was written, read during render and not in an
   * effect: React applies `autoFocus` in the commit phase, before any effect here runs, so
   * an effect reading `document.activeElement` finds the dialog's own cancel button and
   * would then "restore" focus to a control it is about to unmount.
   */
  const opener = useRef<Element | null>(null)
  if (opener.current === null) {
    opener.current = document.activeElement
  }

  // Keyed on presence: leaving runs the cleanup — listener off, focus given back — without waiting for the exit.
  useEffect(() => {
    if (!present) return
    const node = box.current
    // A fallback, never a preference: `autoFocus` has already run by now and put focus on
    // the safe control. This only fires when nothing inside took it.
    if (node !== null && !node.contains(document.activeElement)) {
      // The first control, not the box. Focusing the box drew the focus ring around the whole
      // panel — a ring that says everything is selected and points at nothing a key can
      // operate. The close button is first in every box, so this lands on the way out; a
      // field that asked for focus with `autoFocus` already has it and is left alone. The
      // box stays the fallback for the rare moment focus falls out of it.
      ;(node.querySelector<HTMLElement>(FOCUSABLE) ?? node).focus()
    }

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        close.current()
        return
      }
      if (event.key !== 'Tab') return
      const active = document.activeElement
      const current = node
      if (current === null) return
      const focusable = Array.from(current.querySelectorAll<HTMLElement>(FOCUSABLE))
      if (!current.contains(active) || active === current) {
        // Focus is not on a control in here: either outside the dialog altogether, or on
        // the box itself, which is where it is parked while every control is disabled by
        // the action one of them started. Put it on a control rather than making the user
        // tab in from the top of the document.
        ;(focusable[0] ?? current).focus()
        event.preventDefault()
        return
      }
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (first === undefined || last === undefined) return
      if (event.shiftKey && active === first) {
        last.focus()
        event.preventDefault()
      } else if (!event.shiftKey && active === last) {
        first.focus()
        event.preventDefault()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      // Back to whatever opened this. A dialog that closes onto `<body>` costs a keyboard
      // user their place in the page, and there is no reason for them to hunt for it.
      const chosen = giveBack.current?.() ?? null
      if (chosen !== null) {
        chosen.focus()
        chosen.scrollIntoView?.({ block: 'nearest' })
        return
      }
      const trigger = opener.current
      if (trigger instanceof HTMLElement && document.contains(trigger)) {
        trigger.focus()
      }
    }
  }, [present])

  return createPortal(
    <Fade
      // Under `sm` a dialog is a sheet from the bottom edge (DESIGN.md § Dialogs); the quick look's stage stays centred.
      className={`fixed inset-0 z-[var(--z-overlay)] flex justify-center bg-black/60 ${
        size === 'stage' ? 'items-center p-4 sm:p-6' : 'items-end sm:items-center sm:p-6'
      } ${present ? '' : 'pointer-events-none'}`}
      /*
        The scrim dismisses. `event.target === event.currentTarget` so only the scrim itself
        counts — a click that started inside the box and released out here is a drag, not a
        dismissal, and closing on it loses whatever the person was doing.
      */
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <Panel
        ref={box}
        // Not a dialog any more while it leaves: the rig finds dialogs by these, and a hand-off (Import to From a link)
        // has both in <body> for the exit's 120 ms, the leaving one first.
        role={present ? 'dialog' : undefined}
        aria-modal={present ? 'true' : undefined}
        aria-labelledby={titleId}
        // `max-h-full` and `overflow-y-auto`: a box taller than the window — the quick look with its
        // 3D view, on a laptop screen — scrolls inside it, rather than centring its title and Close
        // out above the top edge where nothing can reach them.
        //
        // Focusable only programmatically: it holds focus while every control inside is
        // disabled, which is the window in which focus would otherwise be nowhere.
        tabIndex={-1}
        aria-hidden={present ? undefined : true}
        inert={!present}
        onBlur={(event) => {
          const current = box.current
          if (current !== null && !leaving.current && !current.contains(event.relatedTarget)) {
            current.focus()
          }
        }}
        className={
          size === 'stage'
            ? 'flex h-[min(88vh,56rem)] max-h-full w-[min(92vw,80rem)] flex-col overflow-y-auto overscroll-contain rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-overlay'
            : `max-h-[92vh] w-full sm:max-h-full ${size === 'wide' ? 'sm:max-w-2xl' : 'sm:max-w-md'} overflow-y-auto overscroll-contain rounded-t-md border border-b-0 border-[var(--color-border)] bg-[var(--color-surface)] p-4 pb-[max(1rem,env(safe-area-inset-bottom))] shadow-overlay sm:rounded-md sm:border-b sm:pb-4`
        }
      >
        <div className="flex items-start justify-between gap-4">
          {/* Scope: 15px/600 in Quicklime, the one heading a dialog has. */}
          <h2 id={titleId} className="text-[15px] leading-snug font-semibold text-[var(--color-bright)]">
            {title}
          </h2>
          {/*
            Escape worked and said so nowhere, so every dialog here was keyboard-only to
            dismiss and impossible on touch. 24px square: WCAG 2.2 SC 2.5.8's minimum, which
            PRODUCT.md commits to.
          */}
          <button
            type="button"
            onClick={onClose}
            aria-label={strings.dialog.close}
            className="ease-mechanical -m-1 flex h-6 w-6 shrink-0 items-center justify-center rounded text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]"
          >
            <Icon name="close" />
          </button>
        </div>
        {children}
      </Panel>
    </Fade>,
    document.body,
  )
}

