import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  fetchWallSetting,
  saveWallSetting,
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
import { Presence } from '../lib/motion-react'
import { strings } from '../lib/strings'
import { FACT, HEADLINE, HINT, LEAD, QUIET, SECTION, SECTION_TITLE } from '../components/Page'
import { AppFrame } from '../components/AppFrame'
import { AppSkeleton } from '../components/Skeleton'
import { FolderPicker } from '../components/FolderPicker'
import { currentUser } from '../lib/session'
import type { SiteCheck, SiteSetting } from '../lib/types'

export const Route = createFileRoute('/settings')({
  component: SettingsPage,
})

/** A site row's Save once there is a key to save: `QUIET`'s size, standing (DESIGN.md § Buttons). */
const READY =
  'ease-mechanical flex min-h-8 flex-none items-center rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-bg)] px-3 text-xs font-semibold whitespace-nowrap text-[var(--color-bright)] duration-[var(--duration-fast)] hover:-translate-y-px'

/** A field beside `QUIET` controls, at their 32px. */
const FIELD =
  'min-h-8 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] bg-[var(--color-raised)] px-2 py-1 text-sm text-[var(--color-text)]'

/** A fact's name, over its value. */
const KEY = 'text-xs text-[var(--color-muted)]'

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
        <section className={SECTION} aria-labelledby="sites-heading">
          <h3 id="sites-heading" className={SECTION_TITLE}>
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
              <ul role="list" className="flex flex-col divide-y divide-[var(--color-border)]">
                {settings.data.sites.map((site) => (
                  <SiteRow key={site.site} site={site} enabled={settings.data.enabled} />
                ))}
              </ul>
            </>
          )}
        </section>
        <ImportSettingsSection />
        <ServerSection />
        <WallSection />
        <ModelsFolderSection />
      </section>
    </AppFrame>
  )
}

/** The sign-in wall (A28): the admin turns the library's pictures on the sign-in page on or off for everybody. */
function WallSection() {
  const queryClient = useQueryClient()
  const wall = useQuery({ queryKey: ['wall-setting'], queryFn: fetchWallSetting })
  const admin = currentUser()?.isAdmin ?? true
  const save = useMutation({
    mutationFn: saveWallSetting,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['wall-setting'] }),
  })
  if (wall.data === undefined) return null
  return (
    <section className={SECTION} aria-labelledby="wall-heading">
      <h3 id="wall-heading" className={SECTION_TITLE}>
        {strings.settings.wallHeading}
      </h3>
      <p className={`mt-2 max-w-prose ${HINT}`}>{strings.settings.wallLead}</p>
      <label className="mt-4 flex min-h-6 items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={save.isPending ? save.variables : wall.data.on}
          disabled={!admin || save.isPending}
          onChange={(event) => save.mutate(event.target.checked)}
          className="size-4 accent-[var(--color-dim)]"
        />
        {strings.settings.wallSwitch}
      </label>
      {admin ? null : <p className={`mt-1 ${HINT}`}>{strings.settings.wallLocked}</p>}
      {save.isError ? (
        <p role="alert" className="mt-3 text-sm text-[var(--color-text)]">
          {save.error.message}
        </p>
      ) : null}
    </section>
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
  const movable = locked === null && admin
  const wanted = Number(port)
  return (
    <>
      {move.isPending ? (
        <div className="fixed inset-0 z-50">
          <AppSkeleton label={strings.settings.moving(wanted)} />
        </div>
      ) : null}
      <section className={SECTION} aria-labelledby="server-heading">
        <h3 id="server-heading" className={SECTION_TITLE}>
          {strings.settings.serverHeading}
        </h3>
        {!movable ? (
          /*
            Nothing here can change it, so it is a fact rather than a field: an Edge border promises you can type (A17's
            audit found the locked port drawn as an input).
          */
          <dl className="mt-4 flex flex-col gap-1.5">
            <dt className={KEY}>{strings.settings.port}</dt>
            <dd className={FACT}>{server.data.port}</dd>
          </dl>
        ) : (
          <form
            className="mt-4 flex flex-wrap items-end gap-2"
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
                onChange={(event) => setPort(event.target.value)}
                className={`${FIELD} tabular w-32`}
              />
            </label>
            <button type="submit" disabled={port === '' || move.isPending} className={port === '' ? QUIET : READY}>
              {strings.settings.movePort}
            </button>
          </form>
        )}
        <p className={`${HINT} mt-2`}>
          {locked !== null
            ? strings.settings.portLocked(locked)
            : admin
              ? strings.settings.portHint
              : strings.settings.portAdminsOnly}
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
      </section>
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
    <section className={SECTION} aria-labelledby="import-heading">
      <h3 id="import-heading" className={SECTION_TITLE}>
        {strings.settings.importHeading}
      </h3>
      <label className="mt-4 flex items-start gap-3 text-sm">
        <input
          type="checkbox"
          checked={current.data?.fetchDetails ?? false}
          disabled={current.isPending || save.isPending}
          onChange={(event) => save.mutate(event.target.checked)}
          className="mt-0.5 size-4 accent-[var(--color-dim)]"
        />
        <span className="flex flex-col gap-1">
          {strings.settings.fetchDetails}
          <span className={HINT}>{strings.settings.fetchDetailsHint}</span>
        </span>
      </label>
    </section>
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
    <li className="flex flex-col gap-2 py-4">
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
            className={`${FIELD} min-w-0 flex-1 font-mono`}
          />
          <button type="submit" disabled={!typed || save.isPending} className={typed ? READY : QUIET}>
            {strings.settings.save}
          </button>
          <button
            type="button"
            disabled={!enabled || check.isPending || (!typed && site.hint === null)}
            onClick={() => check.mutate()}
            className={QUIET}
          >
            {checkLabel}
          </button>
          {site.hint === null ? null : (
            <button type="button" disabled={remove.isPending} onClick={() => remove.mutate()} className={QUIET}>
              {strings.settings.remove}
            </button>
          )}
        </form>
      ) : (
        <div className="flex">
          <button type="button" disabled={!enabled || check.isPending} onClick={() => check.mutate()} className={QUIET}>
            {checkLabel}
          </button>
        </div>
      )}
      {help === undefined ? null : <p className={HINT}>{help}</p>}
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
      <section className={SECTION} aria-labelledby="folders-heading">
        <h3 id="folders-heading" className={SECTION_TITLE}>
          {strings.settings.foldersHeading}
        </h3>
        <dl className="mt-4 flex flex-col gap-1.5">
          <dt className={KEY}>{strings.settings.modelsFolder}</dt>
          <dd className="flex flex-wrap items-center gap-2">
            {/* A sentence when the path is not known: words are not figures (The Copyable Mono Rule). */}
            {data.path === null ? (
              <span className="text-sm text-[var(--color-text)]">{strings.settings.modelsFolderUnknown}</span>
            ) : (
              <span className={FACT}>{data.path}</span>
            )}
            {data.canChoose && locked === null && admin ? (
              <button type="button" onClick={() => setPicking(true)} className={QUIET}>
                {strings.folderPicker.choose}
              </button>
            ) : null}
          </dd>
          <dd className={HINT}>
            {locked !== null
              ? strings.settings.modelsFolderLocked(locked)
              : !data.canChoose
                ? strings.settings.modelsFolderFixed
                : admin
                  ? strings.settings.modelsFolderHint
                  : strings.settings.modelsFolderAdminsOnly}
          </dd>
        </dl>
        <Presence>
          {picking ? (
            <FolderPicker
              key="picker"
              title={strings.folderPicker.modelsTitle}
              start={data.path}
              onChoose={(path) => {
                setPicking(false)
                choose.mutate(path)
              }}
              onCancel={() => setPicking(false)}
            />
          ) : null}
        </Presence>
        {choose.isError ? (
          <p role="alert" className="mt-2 max-w-prose text-sm text-[var(--color-text)]">
            {choose.error.message}
          </p>
        ) : null}
        {lost ? (
          <p role="alert" className="mt-2 max-w-prose text-sm text-[var(--color-text)]">
            {strings.settings.restartLost}
          </p>
        ) : null}
        <dl className="mt-6 flex flex-col gap-1.5">
          <dt className={KEY}>{strings.settings.store}</dt>
          <dd>
            {data.store === null ? (
              <span className="text-sm text-[var(--color-text)]">{strings.settings.storeUnknown}</span>
            ) : (
              <span className={`${FACT} inline-block`}>{data.store}</span>
            )}
          </dd>
          <dd className={HINT}>{strings.settings.storeHint}</dd>
        </dl>
      </section>
    </>
  )
}
