import type { ReactNode } from 'react'
import { strings } from '../../lib/strings'
import type { StoredWidget } from '../../lib/dashboard'
import type { Widget, WidgetResult, WidgetValue } from '../../lib/types'
import type { Limits, Size } from './layout'
import {
  DuplicatesBody,
  FacetBody,
  InstanceStorageBody,
  PartsBody,
  QueueBody,
  StorageBody,
} from './widgets'

/**
 * Every widget kind the web can draw, keyed by the Rust union.
 *
 * **This is the shape `phase-6.md` fixes, and the shape is the point.** `Widget` is a serde enum
 * tagged by `kind` in `crates/lapidary-api/src/dashboard.rs`, exported by ts-rs, and the enum is
 * the dashboard's config schema. Keying this table by `Widget['kind']` means an eighth kind added
 * in Rust fails `tsc` here — a missing property on a mapped type — so it cannot ship with the api
 * able to resolve something the web renders as a blank panel. It is written as a mapped type
 * rather than `Record<Widget['kind'], WidgetSpec>` for one reason: the mapped form carries `K`
 * into each entry, so `recent`'s body is typed against `PartCard[]` and `queue`'s against
 * `QueueSummary` instead of both taking the whole union and narrowing it again by hand.
 *
 * Sizes are in grid cells: twelve columns across, and one row is `--row` tall (see `Board.tsx`).
 * `min` and `max` are what the keyboard resize and a stored layout are held to, and `layout.ts`
 * takes them as an argument — that module knows nothing about this one.
 *
 * `fields` is the settings form, declared rather than drawn: every kind's form is a subset of the
 * same four controls, so `AddWidget.tsx` renders the ones a kind names. Seven form components
 * would be seven places for "which library" to be spelled differently.
 */

/** A control a kind's settings form offers. */
export type Field = 'library' | 'filter' | 'facet' | 'limit'

type ValueOf<K extends Widget['kind']> = Extract<WidgetValue, { kind: K }>['value']
type WidgetOf<K extends Widget['kind']> = Extract<Widget, { kind: K }>

export type WidgetSpec<K extends Widget['kind'] = Widget['kind']> = Limits & {
  /** What the kind is called, in its heading and in the Add menu. */
  label: string
  /** The size a new one of these is added at. */
  size: Size
  fields: readonly Field[]
  body: (value: ValueOf<K>, widget: WidgetOf<K>) => ReactNode
}

export const registry: { [K in Widget['kind']]: WidgetSpec<K> } = {
  storage: {
    label: strings.dashboard.storageLabel,
    min: { w: 3, h: 2 },
    max: { w: 12, h: 3 },
    size: { w: 4, h: 2 },
    fields: ['library'],
    body: (value) => <StorageBody value={value} />,
  },
  instanceStorage: {
    label: strings.dashboard.instanceStorageLabel,
    min: { w: 4, h: 2 },
    max: { w: 12, h: 4 },
    size: { w: 8, h: 2 },
    fields: [],
    body: (value) => <InstanceStorageBody value={value} />,
  },
  recent: {
    label: strings.dashboard.recentLabel,
    min: { w: 3, h: 2 },
    max: { w: 8, h: 8 },
    size: { w: 4, h: 4 },
    fields: ['library', 'limit'],
    body: (value) => <PartsBody parts={value} />,
  },
  savedFilter: {
    label: strings.dashboard.savedFilterLabel,
    min: { w: 3, h: 2 },
    max: { w: 8, h: 8 },
    size: { w: 4, h: 4 },
    fields: ['library', 'filter', 'limit'],
    body: (value) => <PartsBody parts={value.parts} />,
  },
  facet: {
    label: strings.dashboard.facetLabel,
    min: { w: 2, h: 2 },
    max: { w: 6, h: 8 },
    size: { w: 3, h: 4 },
    fields: ['library', 'facet', 'limit'],
    body: (value, widget) => <FacetBody values={value} facet={widget.facet} />,
  },
  queue: {
    label: strings.dashboard.queueLabel,
    min: { w: 2, h: 2 },
    max: { w: 6, h: 3 },
    size: { w: 4, h: 2 },
    fields: ['library'],
    body: (value) => <QueueBody value={value} />,
  },
  duplicates: {
    label: strings.dashboard.duplicatesLabel,
    min: { w: 3, h: 2 },
    max: { w: 6, h: 4 },
    size: { w: 4, h: 2 },
    fields: ['library'],
    body: (value, widget) => <DuplicatesBody value={value} library={widget.library} />,
  },
}

/** The kinds in the order the Add menu offers them: the two cheapest first. */
export const KINDS = Object.keys(registry) as Widget['kind'][]

/** A kind's limits, for `layout.ts`, which is given them rather than reading this table. */
export function limitsOf(widget: Widget): Limits {
  const spec = registry[widget.kind]
  return { min: spec.min, max: spec.max }
}

/**
 * A widget and the value that answered it, carrying their common `kind` at the top level.
 *
 * The tag has to be hoisted: TypeScript narrows a union on its own discriminant, not on a
 * nested one, so a `switch` on `pair.widget.kind` would leave `pair.value` the whole union and
 * every renderer would need a cast. With the kind here, one `switch` narrows both.
 */
type Drawable = { [K in Widget['kind']]: { kind: K; widget: WidgetOf<K>; value: ValueOf<K> } }[Widget['kind']]

/**
 * Draw one widget's value.
 *
 * The single cast in this feature, and the check in front of it is why it is sound: `WidgetValue`
 * is tagged by the same `kind` as the `Widget` it answers, so a value whose kind does not match
 * is a server that broke that promise — drawn as nothing rather than handed to the wrong
 * renderer, which would read `value.parts` off a `QueueSummary`.
 */
export function drawValue(widget: Widget, value: WidgetValue): ReactNode {
  if (widget.kind !== value.kind) return null
  const pair = { kind: widget.kind, widget, value: value.value } as Drawable
  switch (pair.kind) {
    case 'storage':
      return registry.storage.body(pair.value, pair.widget)
    case 'instanceStorage':
      return registry.instanceStorage.body(pair.value, pair.widget)
    case 'recent':
      return registry.recent.body(pair.value, pair.widget)
    case 'savedFilter':
      return registry.savedFilter.body(pair.value, pair.widget)
    case 'facet':
      return registry.facet.body(pair.value, pair.widget)
    case 'queue':
      return registry.queue.body(pair.value, pair.widget)
    case 'duplicates':
      return registry.duplicates.body(pair.value, pair.widget)
  }
  // Exhaustive: an eighth kind in Rust makes `pair` something other than `never` here, and this
  // line is the second place — after the table above — that refuses to compile until it is drawn.
  const unhandled: never = pair
  return unhandled
}

/**
 * A widget's heading.
 *
 * The library's name is the one stored when the widget was added, because no `WidgetValue` but
 * `FilteredParts` names its library and two storage panels for two libraries would otherwise be
 * the same panel twice. A saved filter is the exception and uses the name the server just sent,
 * which is what `FilteredParts.name` is for: renamed since the layout was saved, it shows the
 * new one.
 */
export function widgetTitle(stored: StoredWidget, result: WidgetResult | undefined): string {
  const spec = registry[stored.widget.kind]
  const named =
    result?.status === 'ok' && result.value.kind === 'savedFilter' ? result.value.value.name : spec.label
  return stored.libraryName === null ? named : strings.dashboard.inLibrary(named, stored.libraryName)
}
