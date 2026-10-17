import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  checkSiteKey,
  clearSiteKey,
  fetchImportSettings,
  fetchSiteSettings,
  saveImportSettings,
  saveSiteKey,
} from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import type { SiteCheck, SiteSetting } from '../lib/types'

export const Route = createFileRoute('/settings')({
  component: SettingsPage,
})

const BUTTON =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50'

/**
 * The keys for the sites a model can be imported from (F3). The whole installation's, not one
 * library's, so it carries no library.
 */
export function SettingsPage() {
  const settings = useQuery({ queryKey: ['site-settings'], queryFn: fetchSiteSettings })
  return (
    <AppFrame current="siteSettings">
      <section className="max-w-3xl">
        <title>{strings.titles.settings}</title>
        <h2 className={HEADLINE}>{strings.settings.title}</h2>
        <p className={LEAD}>{strings.settings.lead}</p>
        <h3 className="mt-8 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
          {strings.settings.sitesHeading}
        </h3>
        {settings.isPending ? (
          <p className="mt-4 text-[var(--color-muted)]">{strings.settings.loading}</p>
        ) : settings.isError ? (
          <p role="alert" className="mt-4 max-w-prose text-[var(--color-muted)]">
            {strings.settings.failed}
          </p>
        ) : (
          <>
            {settings.data.enabled ? null : (
              <p role="status" className="mt-4 max-w-prose text-[var(--color-muted)]">
                {strings.settings.switchedOff}
              </p>
            )}
            <ul role="list" className="mt-3 flex flex-col border-t border-[var(--color-border)]">
              {settings.data.sites.map((site) => (
                <SiteRow key={site.site} site={site} enabled={settings.data.enabled} />
              ))}
            </ul>
          </>
        )}
        <ImportSettingsSection />
      </section>
    </AppFrame>
  )
}

/** How Import starts (A2): whether a file added with a page link fetches the page's details. */
function ImportSettingsSection() {
  const queryClient = useQueryClient()
  const current = useQuery({ queryKey: ['import-settings'], queryFn: fetchImportSettings })
  const save = useMutation({
    mutationFn: (fetchDetails: boolean) => saveImportSettings({ fetchDetails }),
    onSuccess: (saved) => queryClient.setQueryData(['import-settings'], saved),
  })
  return (
    <>
      <h3 className="mt-10 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {strings.settings.importHeading}
      </h3>
      <label className="mt-3 flex items-start gap-3 text-sm">
        <input
          type="checkbox"
          checked={current.data?.fetchDetails ?? false}
          disabled={current.isPending || save.isPending}
          onChange={(event) => save.mutate(event.target.checked)}
          className="mt-0.5 size-4 accent-[var(--color-accent)]"
        />
        <span className="flex flex-col gap-1">
          {strings.settings.fetchDetails}
          <span className="text-xs text-[var(--color-dim)]">{strings.settings.fetchDetailsHint}</span>
        </span>
      </label>
    </>
  )
}

/**
 * One site: what is saved, a field for a new key, and a check. **Check** tries the key typed in
 * the field without saving it, or the saved key when the field is empty, so a key can be proved
 * before it replaces one that works.
 */
function SiteRow({ site, enabled }: { site: SiteSetting; enabled: boolean }) {
  const queryClient = useQueryClient()
  const [key, setKey] = useState('')
  const [answer, setAnswer] = useState<SiteCheck | null>(null)
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['site-settings'] })
  const save = useMutation({
    mutationFn: () => saveSiteKey(site.site, key.trim()),
    onSuccess: async () => {
      setKey('')
      setAnswer(null)
      await refresh()
    },
  })
  const remove = useMutation({
    mutationFn: () => clearSiteKey(site.site),
    onSuccess: async () => {
      setAnswer(null)
      await refresh()
    },
  })
  const check = useMutation({
    mutationFn: () => checkSiteKey(site.site, key.trim() === '' ? undefined : key.trim()),
    onSuccess: async (result) => {
      setAnswer(result)
      await refresh()
    },
  })
  const typed = key.trim() !== ''
  const status =
    site.hint === null
      ? strings.settings.noneSaved
      : site.checkedOk === null || site.checkedAt === null
        ? strings.settings.saved(site.hint)
        : `${strings.settings.saved(site.hint)} ${strings.settings.lastCheck(site.checkedOk, site.checkedAt)}`
  const checkLabel = check.isPending
    ? strings.settings.checking
    : !site.needsKey
      ? strings.settings.checkSite
      : typed || site.hint === null
        ? strings.settings.check
        : strings.settings.checkSaved
  const help = strings.settings.keyHelp[site.site]
  const failure = save.isError
    ? strings.settings.saveFailed
    : check.isError
      ? strings.settings.checkFailed
      : null
  return (
    <li className="flex flex-col gap-2 border-b border-[var(--color-border)] py-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h4 className="text-sm font-semibold text-[var(--color-bright)]">{site.name}</h4>
        <p className="text-xs text-[var(--color-muted)]">{site.needsKey ? status : strings.settings.noKeyNeeded}</p>
      </div>
      {site.needsKey ? (
        <form
          className="flex flex-wrap items-center gap-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (typed && !save.isPending) save.mutate()
          }}
        >
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={key}
            onChange={(event) => setKey(event.target.value)}
            aria-label={strings.settings.keyLabel(site.name)}
            className="min-w-0 flex-1 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 font-mono text-sm"
          />
          <button type="submit" disabled={!typed || save.isPending} className={BUTTON}>
            {strings.settings.save}
          </button>
          <button
            type="button"
            disabled={!enabled || check.isPending || (!typed && site.hint === null)}
            onClick={() => check.mutate()}
            className={BUTTON}
          >
            {checkLabel}
          </button>
          {site.hint === null ? null : (
            <button type="button" disabled={remove.isPending} onClick={() => remove.mutate()} className={BUTTON}>
              {strings.settings.remove}
            </button>
          )}
        </form>
      ) : (
        <div>
          <button type="button" disabled={!enabled || check.isPending} onClick={() => check.mutate()} className={BUTTON}>
            {checkLabel}
          </button>
        </div>
      )}
      {help === undefined ? null : <p className="max-w-prose text-xs text-[var(--color-muted)]">{help}</p>}
      {answer === null ? null : (
        <p role="status" className="max-w-prose text-sm text-[var(--color-muted)]">
          {answer.message}
        </p>
      )}
      {failure === null ? null : (
        <p role="alert" className="max-w-prose text-sm text-[var(--color-muted)]">
          {failure}
        </p>
      )}
    </li>
  )
}
