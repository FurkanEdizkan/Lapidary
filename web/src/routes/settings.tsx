import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  chooseModelsFolder,
  checkSiteKey,
  clearSiteKey,
  fetchImportSettings,
  fetchModelsFolder,
  fetchServerPort,
  fetchSiteSettings,
  followPort,
  followRestart,
  movePort,
  saveImportSettings,
  saveSiteKey,
} from '../lib/api'
import { strings } from '../lib/strings'
import { HEADLINE, LEAD } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import { AppSkeleton } from '../components/Skeleton'
import { FolderPicker } from '../components/FolderPicker'
import { currentUser } from '../lib/session'
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
        <ServerSection />
        <ModelsFolderSection />
      </section>
    </AppFrame>
  )
}

/**
 * The port (A5): fixed by deploy/.env, or this install's own and movable here. Moving shows the app's
 * skeleton while the page follows the app to its new port.
 */
function ServerSection() {
  const server = useQuery({ queryKey: ['server-port'], queryFn: fetchServerPort })
  const [port, setPort] = useState('')
  const [lost, setLost] = useState<number | null>(null)
  const admin = currentUser()?.isAdmin ?? true
  const move = useMutation({
    mutationFn: async (to: number) => {
      await movePort(to)
      if (!(await followPort(to))) setLost(to)
    },
  })
  if (server.data === undefined) return null
  const locked = server.data.lockedBy
  const wanted = Number(port)
  return (
    <>
      {move.isPending ? (
        <div className="fixed inset-0 z-50">
          <AppSkeleton label={strings.settings.moving(wanted)} />
        </div>
      ) : null}
      <h3 className="mt-10 text-xs font-medium tracking-widest text-[var(--color-muted)] uppercase">
        {strings.settings.serverHeading}
      </h3>
      <form
        className="mt-3 flex flex-wrap items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          if (Number.isInteger(wanted) && wanted > 0 && !move.isPending) move.mutate(wanted)
        }}
      >
        <label className="flex flex-col gap-1 text-xs text-[var(--color-muted)]">
          {strings.settings.port}
          <input
            type="number"
            min={1024}
            max={65535}
            placeholder={String(server.data.port)}
            value={port}
            disabled={locked !== null || !admin}
            onChange={(event) => setPort(event.target.value)}
            className="w-32 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1.5 text-sm text-[var(--color-text)] tabular"
          />
        </label>
        {locked === null && admin ? (
          <button type="submit" disabled={port === '' || move.isPending} className={BUTTON}>
            {strings.settings.movePort}
          </button>
        ) : null}
      </form>
      <p className="mt-2 max-w-prose text-xs text-[var(--color-dim)]">
        {locked !== null ? strings.settings.portLocked(locked) : admin ? strings.settings.portHint : strings.settings.portAdminsOnly}
      </p>
      {move.isError ? (
        <p role="alert" className="mt-2 text-sm text-[var(--color-text)]">
          {move.error.message}
        </p>
      ) : null}
      {lost === null ? null : (
        <p role="alert" className="mt-2 text-sm text-[var(--color-text)]">
          {strings.settings.moveLost(lost)}
        </p>
      )}
    </>
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

/**
 * The models folder (A6): fixed by deploy/.env, or chosen here from the machine's folders. Choosing one
 * restarts Lapidary on it, behind the app's skeleton, and the page comes back signed in.
 */
function ModelsFolderSection() {
  const folder = useQuery({ queryKey: ['models-folder'], queryFn: () => fetchModelsFolder() })
  const [picking, setPicking] = useState(false)
  const [lost, setLost] = useState(false)
  const admin = currentUser()?.isAdmin ?? true
  const choose = useMutation({
    mutationFn: async (path: string) => {
      if (!(await chooseModelsFolder({ path, store: null }))) return
      const port = window.location.port === '' ? (window.location.protocol === 'https:' ? 443 : 80) : Number(window.location.port)
      if (!(await followRestart(port))) setLost(true)
    },
  })
  if (folder.data === undefined) return null
  const data = folder.data
  const locked = data.lockedBy
  return (
    <>
      {choose.isPending && choose.variables !== undefined ? (
        <div className="fixed inset-0 z-50">
          <AppSkeleton label={strings.settings.restarting(choose.variables)} />
        </div>
      ) : null}
      <div className="mt-6 flex flex-col gap-1.5">
        <p className="text-xs text-[var(--color-muted)]">{strings.settings.modelsFolder}</p>
        <p className="font-mono text-[13px] break-all">{data.path ?? strings.settings.modelsFolderUnknown}</p>
        <p className="max-w-prose text-xs text-[var(--color-dim)]">
          {locked !== null
            ? strings.settings.modelsFolderLocked(locked)
            : !data.canChoose
              ? strings.settings.modelsFolderFixed
              : admin
                ? strings.settings.modelsFolderHint
                : strings.settings.modelsFolderAdminsOnly}
        </p>
        {data.canChoose && locked === null && admin ? (
          <button type="button" onClick={() => setPicking(true)} className={`${BUTTON} self-start`}>
            {strings.folderPicker.choose}
          </button>
        ) : null}
        {picking ? (
          <FolderPicker
            title={strings.folderPicker.modelsTitle}
            start={data.path}
            onChoose={(path) => {
              setPicking(false)
              choose.mutate(path)
            }}
            onCancel={() => setPicking(false)}
          />
        ) : null}
        {choose.isError ? (
          <p role="alert" className="max-w-prose text-sm text-[var(--color-text)]">
            {choose.error.message}
          </p>
        ) : null}
        {lost ? (
          <p role="alert" className="max-w-prose text-sm text-[var(--color-text)]">
            {strings.settings.restartLost}
          </p>
        ) : null}
      </div>
      <div className="mt-6 flex flex-col gap-1.5">
        <p className="text-xs text-[var(--color-muted)]">{strings.settings.store}</p>
        <p className="font-mono text-[13px] break-all">{data.store ?? strings.settings.storeUnknown}</p>
        <p className="max-w-prose text-xs text-[var(--color-dim)]">{strings.settings.storeHint}</p>
      </div>
    </>
  )
}
