import { strings } from '../lib/strings'
import { Popover, usePopover } from './TopBar'
import { CARD_SIZES, LAYOUTS, PAGE_SIZES } from '../lib/preferences'
import type { CardSize, Layout, PageSize } from '../lib/preferences'

/**
 * How the grid draws itself, in a popover under the bar — `v2`'s View menu.
 *
 * # Why this replaces two selects rather than joining them
 *
 * `GridSettings` was a row of native `<select>`s sitting above the grid, chosen because a
 * native select is keyboard-operable and screen-reader announced for free. That argument
 * still holds for the page size, which stays a select here. It does not hold for the
 * layout and the card size: both are *previews of themselves* — the whole reason to touch
 * either is to see the grid change — and a select closes its own popup on the click that
 * changes the value, so a person adjusting a card size through one is choosing blind.
 *
 * The trade is that a segmented control and a slider are not free the way a select is, so
 * both are spelled out below: the group carries `role="group"` and a label, the buttons
 * carry `aria-pressed`, and the slider is a real `input type="range"` with its value
 * mirrored in text rather than a set of divs with pointer handlers.
 *
 * # Everything here is per library, in this browser
 *
 * The same scope `preferences.ts` has always had, and the menu says so at its foot rather
 * than leaving a person to discover it. There is no user table in Phase 1, so a server-side
 * preference would make one operator's choice everybody's.
 */
export function ViewMenu({
  layout,
  onLayout,
  cardSize,
  onCardSize,
  namesAlways,
  onNamesAlways,
  pageSize,
  onPageSize,
}: {
  layout: Layout
  onLayout: (layout: Layout) => void
  cardSize: CardSize
  onCardSize: (size: CardSize) => void
  namesAlways: boolean
  onNamesAlways: (on: boolean) => void
  pageSize: PageSize
  onPageSize: (size: PageSize) => void
}) {
  const popover = usePopover()
  /*
    Hoisted out of the JSX, and not for tidiness. `no-bare-strings.test.ts` reads any string
    literal inside a JSX child expression as a label reaching the screen, which `layout !==
    'gallery' ? …` would be — the discriminator is a literal in a child position. The check
    is right to be strict there; this is the shape that satisfies it.
  */
  const gallery = layout === 'gallery'
  return (
    <Popover label={strings.shell.view} {...popover}>
      <Group label={strings.layout.label}>
        {LAYOUTS.map((option) => (
          <Segment
            key={option}
            selected={option === layout}
            onSelect={() => onLayout(option)}
            label={LAYOUT_LABEL[option]}
          />
        ))}
      </Group>

      {/*
        The card size, and it only means something in the gallery. A list row is a row —
        the control would move nothing — so rather than leave a slider that does nothing to
        the thing in front of the user, it is not rendered. Hiding rather than disabling: a
        disabled control invites a person to work out what would enable it, and the answer
        here is "switch layout", which the group directly above already offers.
      */}
      {!gallery ? null : (
        <label className="flex flex-col gap-[7px] pt-1">
          <span className="flex items-baseline justify-between gap-2">
            <span className="text-[11.5px] font-semibold text-[var(--color-muted)]">
              {strings.layout.cardSize}
            </span>
            {/*
              The value in words beside the slider, which is what makes a four-step range
              legible. `aria-hidden` because the input's own value is already announced —
              without it a screen reader reads the size twice, once as a number and once as
              a word.
            */}
            <span aria-hidden="true" className="tabular text-[10.5px] text-[var(--color-dim)]">
              {CARD_SIZE_LABEL[cardSize]}
            </span>
          </span>
          <input
            type="range"
            min={0}
            max={CARD_SIZES.length - 1}
            step={1}
            value={CARD_SIZES.indexOf(cardSize)}
            onChange={(event) => {
              /*
                Indexed into the named scale rather than stored as the number. The index is
                meaningless in `localStorage` — a release that reordered the scale would
                silently resize somebody's grid — so the slider is the only place the two
                representations meet. `?? cardSize` because a range input is a string and a
                value outside the scale must leave the setting where it was.
              */
              const next = CARD_SIZES[Number(event.target.value)]
              onCardSize(next ?? cardSize)
            }}
            /*
              The words, not the index, as the announced value. `aria-valuetext` is what
              stops a screen reader saying "2 of 3" about a control whose values are Small
              through Huge.
            */
            aria-valuetext={CARD_SIZE_LABEL[cardSize]}
            className="w-full accent-[var(--color-accent)]"
          />
        </label>
      )}

      {!gallery ? null : (
        <Toggle
          label={strings.layout.namesAlways}
          hint={strings.layout.namesAlwaysHint}
          on={namesAlways}
          onToggle={() => onNamesAlways(!namesAlways)}
        />
      )}

      <label className="flex items-center justify-between gap-3 border-t border-[var(--color-border)] pt-3">
        <span className="text-[11.5px] font-semibold text-[var(--color-muted)]">
          {strings.grid.pageSize}
        </span>
        {/*
          Still a native select, and for the reason the others stopped being one: this
          setting is not a preview of itself. Changing it refetches, and nobody adjusts it
          back and forth to see what happens.
        */}
        <select
          value={pageSize}
          onChange={(event) => onPageSize(Number(event.target.value) as PageSize)}
          className="rounded-[var(--radius-ctl)] border border-[var(--color-border)] bg-[var(--color-raised)] px-2 py-1.5 text-[11.5px] text-[var(--color-dim)]"
        >
          {PAGE_SIZES.map((size) => (
            <option key={size} value={size}>
              {strings.grid.pageSizeOption(size)}
            </option>
          ))}
        </select>
      </label>

      <p className="tabular text-[9px] tracking-[0.08em] text-[var(--color-muted)] uppercase">
        {strings.shell.viewScope}
      </p>
    </Popover>
  )
}

/**
 * The labels, as lookups rather than ternaries in the JSX.
 *
 * `no-bare-strings.test.ts` reads a string literal inside a JSX child expression as a label
 * reaching the screen — correctly — so `option === 'list' ? … : …` would put the
 * discriminator itself where the check can see it. The same trap `DENSITY_LABEL` and
 * `ShowInFolder` both carry a note about.
 */
const LAYOUT_LABEL: Record<Layout, string> = {
  gallery: strings.layout.gallery,
  list: strings.layout.list,
}

const CARD_SIZE_LABEL: Record<CardSize, string> = {
  small: strings.layout.cardSizeSmall,
  medium: strings.layout.cardSizeMedium,
  large: strings.layout.cardSizeLarge,
  huge: strings.layout.cardSizeHuge,
}

/**
 * A labelled set of mutually exclusive buttons.
 *
 * `role="group"` with a name, and `aria-pressed` on each button — not `radiogroup` with
 * `role="radio"`. A radio group owes the arrow-key roving-tabindex behaviour that ARIA's
 * pattern specifies, and a set of buttons that announces itself as radios without providing
 * it is worse for a keyboard user than one that announces itself as buttons and behaves
 * like them.
 */
function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-[7px]">
      <span
        id={`view-${label}`}
        className="tabular text-[9px] tracking-[0.2em] text-[var(--color-muted)] uppercase"
      >
        {label}
      </span>
      <div role="group" aria-labelledby={`view-${label}`} className="flex gap-[6px]">
        {children}
      </div>
    </div>
  )
}

function Segment({
  label,
  selected,
  onSelect,
}: {
  label: string
  selected: boolean
  onSelect: () => void
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className={`ease-mechanical flex-1 rounded-[var(--radius-ctl)] border px-2 py-[6px] text-[11.5px] font-semibold duration-[var(--duration-fast)] ${
        selected
          ? 'border-[var(--color-accent)] bg-[color-mix(in_oklab,var(--color-accent)_12%,transparent)] text-[var(--color-accent)]'
          : 'border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-edge)] hover:text-[var(--color-text)]'
      }`}
    >
      {label}
    </button>
  )
}

/**
 * The design's switch: a track with a knob that slides.
 *
 * A `button` with `aria-pressed` rather than a checkbox styled out of recognition. The two
 * announce differently — "toggle button, pressed" against "checkbox, checked" — and this is
 * a control that takes effect immediately rather than one a form submits, which is the
 * distinction the button carries and the checkbox does not.
 *
 * The knob moves with `translate`, which is one of the two properties `CLAUDE.md` allows
 * motion on, and the base layer's transition picks it up without a duration written here.
 */
function Toggle({
  label,
  hint,
  on,
  onToggle,
}: {
  label: string
  hint: string
  on: boolean
  onToggle: () => void
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-pressed={on}
      className="ease-mechanical flex w-full items-center justify-between gap-3 rounded-[var(--radius-ctl)] px-[6px] py-[7px] text-left duration-[var(--duration-fast)] hover:bg-[var(--color-raised)]"
    >
      <span className="flex min-w-0 flex-col gap-[2px]">
        <span className="text-xs text-[var(--color-text)]">{label}</span>
        <span className="text-[10.5px] text-[var(--color-muted)]">{hint}</span>
      </span>
      <span
        aria-hidden="true"
        className={`flex h-[16px] w-[28px] flex-none items-center rounded-full p-[2px] ${
          on ? 'bg-[var(--color-accent)]' : 'bg-[var(--color-border)]'
        }`}
      >
        <span
          className={`block size-[12px] rounded-full bg-white ${on ? 'translate-x-[12px]' : ''}`}
        />
      </span>
    </button>
  )
}
