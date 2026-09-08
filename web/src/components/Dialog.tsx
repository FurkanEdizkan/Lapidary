import { useEffect, useId, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

/** Everything inside the box that a Tab can land on. `:not([disabled])` is the point. */
const FOCUSABLE =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

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
 */
export function Dialog({
  title,
  onClose,
  children,
}: {
  title: string
  onClose: () => void
  children: ReactNode
}) {
  const titleId = useId()
  const box = useRef<HTMLDivElement>(null)
  // Every caller passes an inline arrow, so `onClose` is a new function each render. Read
  // through a ref rather than depending on it: an effect keyed on the callback would tear
  // down and re-run on every render, and its cleanup would throw focus back at the trigger
  // while the dialog was still open.
  const close = useRef(onClose)
  useEffect(() => {
    close.current = onClose
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

  useEffect(() => {
    const node = box.current
    // A fallback, never a preference: `autoFocus` has already run by now and put focus on
    // the safe control. This only fires when nothing inside took it.
    if (node !== null && !node.contains(document.activeElement)) {
      node.focus()
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
      const trigger = opener.current
      if (trigger instanceof HTMLElement && document.contains(trigger)) {
        trigger.focus()
      }
    }
  }, [])

  return createPortal(
    <div className="fixed inset-0 z-10 flex items-center justify-center bg-black/60 p-6">
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        // Focusable only programmatically: it holds focus while every control inside is
        // disabled, which is the window in which focus would otherwise be nowhere.
        tabIndex={-1}
        onBlur={(event) => {
          const current = box.current
          if (current !== null && !current.contains(event.relatedTarget)) {
            current.focus()
          }
        }}
        className="w-full max-w-md rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
      >
        <h2 id={titleId} className="text-sm font-medium">
          {title}
        </h2>
        {children}
      </div>
    </div>,
    document.body,
  )
}

