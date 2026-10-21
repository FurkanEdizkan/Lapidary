import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useEffect, useState, type ReactNode } from 'react'
import { BrandMark } from '../components/AppFrame'
import { Doorway, Field, INPUT } from '../components/Doorway'
import { Icon } from '../components/Icon'
import { FACT, HEADLINE, HINT, LEAD, SECTION_TITLE, STANDING } from '../components/Page'
import { LOGIN_PATH, Refused, fetchSetupStatus, runSetup } from '../lib/session'
import {
  chooseModelsFolder,
  fetchImportWorkers,
  fetchModelsFolder,
  followPort,
  followRestart,
  movePort,
  saveImportWorkers,
  saveWallSetting,
} from '../lib/api'
import { FolderPicker } from '../components/FolderPicker'
import { Fade, Indicator, Presence, Rise, Step } from '../lib/motion-react'
import { reduced, tokens } from '../lib/motion-tokens'
import { strings } from '../lib/strings'
import type { SetupStatus } from '../lib/types'

export const Route = createFileRoute('/setup')({
  component: SetupPage,
})

const STEPS = 3
// Which folder picker is open on the server step.
const MODELS = 'models'
const STORE = 'store'
type Picking = typeof MODELS | typeof STORE
const MIN_PASSWORD = 8
/** How many imports may run at once: `IMPORT_WORKERS_RANGE` in lapidary-api's server_port.rs (A41). */
const MIN_WORKERS = 1
const MAX_WORKERS = 8
/** The refusal that is about the setup code itself, shown under that field. */
const WRONG_CODE = 'wrong_code'

/** Back and Skip: quiet, and bounded by Edge like every control a person can press (The Operable Edge Rule). */
const QUIET =
  'ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-2 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]'

/** A folder nobody changes on this step, as Settings sets it: a fact on the well, or a sentence when it is unknown. */
function Fact({ path, unknown }: { path: string | null | undefined; unknown: string }) {
  return path == null ? <p className="text-sm text-[var(--color-text)]">{unknown}</p> : <p className={FACT}>{path}</p>
}

/**
 * The intro plays once per page load (DESIGN.md § Stepper). Set when it ends rather than when it starts, so a mount
 * that is thrown away (StrictMode's first) does not use it up.
 */
let introPlayed = false

/** The mark at the centre; fading as the wizard rises; over. */
type Phase = 'centre' | 'travel' | 'end'

/** Where the wizard is, and which way it last moved, so Back plays the step change in reverse. */
type At = { step: number; direction: 1 | -1 }

/**
 * The first-run wizard (A1): the admin's account, what this server reads and keeps, and the one option
 * worth deciding before anything is imported. Nothing is saved until Finish, so Back loses nothing;
 * a wrong setup code is the server's to say, and sends the person back to the step that has it.
 *
 * A20 puts it in the Doorway's split, after a short intro: the mark at the centre of the screen, which fades as the
 * wizard rises (since A29 nothing draws or travels). A key, a click or Skip ends it at its end state; under reduced motion it
 * never plays.
 */
export function SetupPage() {
  const status = useQuery({ queryKey: ['setup-status'], queryFn: fetchSetupStatus })
  const [intro] = useState(() => !introPlayed && !reduced())
  const [phase, setPhase] = useState<Phase>(intro ? 'centre' : 'end')
  const [skipped, setSkipped] = useState(false)
  const [at, setAt] = useState<At>({ step: 1, direction: 1 })
  // Finish was pressed and answered: the mark fills while the server is followed, and never holds it up.
  const [done, setDone] = useState(false)

  // The clock: centre for `--duration-scene`, then the travel, all inside `--duration-intro`.
  useEffect(() => {
    if (!intro) return
    const timers: number[] = []
    const t = tokens()
    timers.push(window.setTimeout(() => setPhase((now) => (now === 'centre' ? 'travel' : now)), t.scene))
    timers.push(window.setTimeout(() => setPhase('end'), t.intro))
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer))
    }
  }, [intro])

  // Any key or click ends it at once.
  const over = phase === 'end'
  useEffect(() => {
    if (over) return
    const end = () => {
      setSkipped(true)
      setPhase('end')
    }
    window.addEventListener('keydown', end)
    window.addEventListener('pointerdown', end)
    return () => {
      window.removeEventListener('keydown', end)
      window.removeEventListener('pointerdown', end)
    }
  }, [over])

  useEffect(() => {
    if (intro && over) introPlayed = true
  }, [intro, over])

  const centre = phase === 'centre'
  const wizard = status.data?.required === true
  // What arrives after the intro rises in; without one (or skipped) it is simply there.
  const arrive = (children: ReactNode, className?: string) => (
    <Rise className={className} {...(intro && !skipped ? {} : { initial: false })}>
      {children}
    </Rise>
  )

  return (
    <Doorway
      done={done}
      aside={<div className="w-full max-w-md">{wizard && !centre ? arrive(<Stepper step={at.step} />) : null}</div>}
    >
      <title>{strings.titles.setup}</title>
      <Presence>
        {centre ? (
          // Opaque from the first frame; it only fades on the way out.
          <Fade key="intro" initial={false} className="fixed inset-0 z-40 bg-[var(--color-bg)]">
            <div className="flex h-full items-center justify-center">
              <span className="scale-[2]">
                <BrandMark />
              </span>
            </div>
            <button
              type="button"
              onClick={() => {
                setSkipped(true)
                setPhase('end')
              }}
              className={`${QUIET} absolute right-4 bottom-4 lg:right-8 lg:bottom-8`}
            >
              {strings.setup.skip}
            </button>
          </Fade>
        ) : null}
      </Presence>
      {centre
        ? null
        : arrive(
            <>
              <h2 className={HEADLINE}>
                {status.data?.required === false ? strings.setup.alreadyTitle : strings.setup.title}
              </h2>
              {status.isPending ? (
                <p className="mt-6 text-[var(--color-muted)]">{strings.setup.loading}</p>
              ) : status.isError ? (
                <p role="alert" className="mt-6 text-[var(--color-muted)]">
                  {strings.setup.failed}
                </p>
              ) : !status.data.required ? (
                // Already set up: the account is somebody's, and signing in is the way in.
                <AlreadyDone />
              ) : (
                <Wizard
                  status={status.data}
                  step={at.step}
                  direction={at.direction}
                  go={(step) => setAt({ step, direction: step > at.step ? 1 : -1 })}
                  onFinishing={setDone}
                />
              )}
            </>,
            wizard ? STEADY : undefined,
          )}
    </Doorway>
  )
}

function AlreadyDone() {
  return (
    <>
      <p className={LEAD}>{strings.setup.alreadyLead}</p>
      <a href={LOGIN_PATH} className={`${STANDING} mt-8 inline-block whitespace-nowrap`}>
        {strings.login.submit}
      </a>
    </>
  )
}

/** As tall as the tallest step, at `lg`, so the heading stays put while the steps change under it. */
const STEADY = 'lg:min-h-[40rem]'
const CURRENT = 'text-[var(--color-bright)]'
const DONE = 'text-[var(--color-dim)]'
const AHEAD = 'text-[var(--color-muted)]'

/**
 * The steps, beside the rail (DESIGN.md § Stepper): the current one in Quicklime with the grey marker over it, done
 * ones in Ash with a check, the ones ahead in Graphite. At `lg` only; below it the wizard says the same in one line.
 */
function Stepper({ step }: { step: number }) {
  return (
    <ol
      role="list"
      aria-label={strings.setup.stepsLabel}
      className="relative hidden flex-col before:absolute before:inset-y-0 before:left-0 before:w-px before:bg-[var(--color-border)] lg:flex"
    >
      {strings.setup.steps.map((name, index) => {
        const n = index + 1
        const tone = n === step ? CURRENT : n < step ? DONE : AHEAD
        return (
          <li
            key={name}
            aria-current={n === step ? 'step' : undefined}
            className={`relative flex items-center gap-2 py-1.5 pl-5 text-[13px] font-semibold ${tone}`}
          >
            {n === step ? (
              <Indicator id="setup-rail" className="absolute inset-y-0 left-[-0.5px] w-[2px] bg-[var(--color-bright)]" />
            ) : null}
            {name}
            {n < step ? (
              <>
                <Icon name="check" size={12} />
                <span className="sr-only">{strings.setup.stepDone}</span>
              </>
            ) : null}
          </li>
        )
      })}
    </ol>
  )
}

function Wizard({
  status,
  step,
  direction,
  go,
  onFinishing,
}: {
  status: SetupStatus
  step: number
  direction: 1 | -1
  go: (step: number) => void
  onFinishing: (finishing: boolean) => void
}) {
  const [code, setCode] = useState('')
  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState(status.suggestedName ?? '')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [importLinks, setImportLinks] = useState(!status.importLinksForcedOff)
  // The sign-in wall (A28) and how many imports run at once (A41), applied once the account exists.
  const [wall, setWall] = useState(true)
  const running = useQuery({
    queryKey: ['setup-workers', code],
    queryFn: () => fetchImportWorkers(code),
    enabled: step === 3,
    retry: false,
  })
  const [workers, setWorkers] = useState<string | null>(null)
  const asked = Number(workers)
  const newWorkers =
    workers !== null &&
    running.data !== undefined &&
    running.data.lockedBy === null &&
    Number.isInteger(asked) &&
    asked >= MIN_WORKERS &&
    asked <= MAX_WORKERS &&
    asked !== running.data.workers
      ? asked
      : null
  // The port this page was opened on (A5); changing it moves Lapidary there once the account exists.
  const here = window.location.port === '' ? '' : window.location.port
  const [port, setPort] = useState(here)
  // The models folder (A6), chosen here and applied once the account exists: Lapidary restarts on it.
  const folder = useQuery({
    queryKey: ['setup-models-folder', code],
    queryFn: () => fetchModelsFolder(code),
    enabled: step === 2,
    retry: false,
  })
  const [chosen, setChosen] = useState<string | null>(null)
  // And Lapidary's own files (A8): copied to the folder chosen for them, in the same restart.
  const [store, setStore] = useState<string | null>(null)
  const [picking, setPicking] = useState<Picking | null>(null)
  const [problem, setProblem] = useState<string | null>(null)

  const finish = useMutation({
    mutationFn: () =>
      runSetup({
        code,
        username,
        displayName: displayName.trim() === '' ? null : displayName,
        password,
        importLinks: status.importLinksForcedOff ? null : importLinks,
        importWorkers: newWorkers ?? undefined,
      }),
    onSuccess: async () => {
      // The account exists: the done moment, while whatever follows runs. It holds nothing up.
      onFinishing(true)
      const to = Number(port)
      let moved = false
      if (port !== here && Number.isInteger(to) && to >= 1024) {
        // Signed in now, so the move is allowed; a refusal (a fixed or busy port) leaves Lapidary
        // where it is, and the person can move it from Settings.
        try {
          await movePort(to)
          moved = true
        } catch {
          // Stay on this port.
        }
      }
      if (!wall) {
        try {
          await saveWallSetting(false)
        } catch {
          // Left on; Settings turns it off.
        }
      }
      const path = chosen !== null && chosen !== folder.data?.path ? chosen : null
      const kept = store !== null && store !== folder.data?.store ? store : null
      if (path !== null || kept !== null) {
        try {
          // The restart comes back on the saved port, moved or not.
          const restarting = await chooseModelsFolder({ path, store: kept })
          if (restarting && (await followRestart(moved ? to : thisPort(), HOME))) return
        } catch {
          // Refused: Settings shows the folder and why.
          window.location.assign(SETTINGS_PATH)
          return
        }
      }
      if (newWorkers !== null) {
        try {
          // Setup saved the number; the worker takes it when Lapidary starts again.
          const restarting = await saveImportWorkers(newWorkers)
          if (restarting && (await followRestart(moved ? to : thisPort(), HOME))) return
        } catch {
          // Saved either way: it applies at the next start.
        }
      }
      if (moved && (await followPort(to, undefined, HOME))) return
      window.location.assign('/')
    },
    onError: (error) => {
      // Every refusal the server can give is about the first step's fields.
      if (error instanceof Refused) go(1)
    },
  })

  const refusal = finish.error instanceof Refused ? finish.error : null
  // A wrong code goes under the code; anything else the server refuses is about the account as a whole.
  const codeRefused = refusal?.code === WRONG_CODE
  const otherRefusal = refusal !== null && !codeRefused ? refusal.message : null

  const accountReady = () => {
    if (code.trim() === '' || username.trim() === '' || password === '') return strings.setup.missing
    if (password.length < MIN_PASSWORD) return strings.setup.passwordShort
    if (password !== confirm) return strings.setup.passwordsDiffer
    return null
  }

  return (
    <form
      className="mt-2 flex flex-col"
      onSubmit={(event) => {
        event.preventDefault()
        if (step === 1) {
          const wrong = accountReady()
          setProblem(wrong)
          if (wrong === null) {
            finish.reset()
            go(2)
          }
        } else if (step === 2) {
          go(3)
        } else if (!finish.isPending) {
          finish.mutate()
        }
      }}
    >
      {/* Below `lg`, the stepper in one line, its rail a hairline under it. */}
      <div className="lg:hidden">
        <p className="text-xs text-[var(--color-dim)]">
          {strings.setup.step(step, STEPS, strings.setup.steps[step - 1] ?? '')}
        </p>
        <div className="mt-2 h-px bg-[var(--color-border)]">
          <div className="h-px bg-[var(--color-bright)]" style={{ width: `${(step / STEPS) * 100}%` }} />
        </div>
      </div>

      <Presence mode="wait" custom={direction} initial={false}>
        <Step key={step} direction={direction}>
          {step === 1 ? (
            <section className="mt-6 flex flex-col gap-4">
              <h3 className={SECTION_TITLE}>{strings.setup.accountTitle}</h3>
              <p className="-mt-2 text-sm text-[var(--color-dim)]">{strings.setup.accountLead}</p>
              <Field label={strings.setup.codeLabel} hint={strings.setup.codeHint}>
                <input
                  type="text"
                  autoComplete="off"
                  spellCheck={false}
                  value={code}
                  autoFocus={codeRefused}
                  aria-invalid={codeRefused}
                  onChange={(event) => setCode(event.target.value)}
                  className={`${INPUT} tabular tracking-wider uppercase`}
                />
              </Field>
              {codeRefused ? (
                <p role="alert" className="-mt-2 text-sm text-[var(--color-text)]">
                  {finish.error?.message}
                </p>
              ) : null}
              <Field label={strings.setup.usernameLabel} hint={strings.setup.usernameHint}>
                <input
                  type="text"
                  autoComplete="username"
                  autoCapitalize="none"
                  spellCheck={false}
                  maxLength={40}
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  className={INPUT}
                />
              </Field>
              <Field label={strings.setup.displayNameLabel}>
                <input
                  type="text"
                  autoComplete="name"
                  maxLength={80}
                  value={displayName}
                  onChange={(event) => setDisplayName(event.target.value)}
                  className={INPUT}
                />
              </Field>
              <Field label={strings.setup.passwordLabel} hint={strings.setup.passwordHint}>
                <input
                  type="password"
                  autoComplete="new-password"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  className={INPUT}
                />
              </Field>
              <Field label={strings.setup.confirmLabel}>
                <input
                  type="password"
                  autoComplete="new-password"
                  value={confirm}
                  onChange={(event) => setConfirm(event.target.value)}
                  className={INPUT}
                />
              </Field>
            </section>
          ) : step === 2 ? (
            <section className="mt-6 flex flex-col gap-4">
              <h3 className={SECTION_TITLE}>{strings.setup.serverTitle}</h3>
              <p className="-mt-2 text-sm text-[var(--color-dim)]">{strings.setup.serverLead}</p>
              <Field label={strings.setup.port} hint={strings.setup.portHint}>
                <input
                  type="number"
                  min={1024}
                  max={65535}
                  value={port}
                  onChange={(event) => setPort(event.target.value)}
                  className={`${INPUT} tabular w-32`}
                />
              </Field>
              <div className="flex flex-col gap-1.5">
                <p className="text-xs text-[var(--color-muted)]">{strings.setup.modelsFolder}</p>
                <Fact path={chosen ?? folder.data?.path ?? status.hostIngestRoot} unknown={strings.settings.modelsFolderUnknown} />
                <p className={HINT}>
                  {folder.data?.lockedBy
                    ? strings.settings.modelsFolderLocked(folder.data.lockedBy)
                    : folder.data && !folder.data.canChoose
                      ? strings.settings.modelsFolderFixed
                      : strings.setup.modelsFolderHint}
                </p>
                {folder.isError ? (
                  // A stale setup code, most often: the server's sentence says so.
                  <p role="alert" className="text-sm text-[var(--color-text)]">
                    {folder.error.message}
                  </p>
                ) : null}
                {folder.data?.canChoose && folder.data.lockedBy === null ? (
                  <Choose
                    title={strings.folderPicker.modelsTitle}
                    open={picking === MODELS}
                    start={chosen ?? folder.data.path}
                    code={code}
                    onOpen={() => setPicking(MODELS)}
                    onChoose={(path) => {
                      setChosen(path)
                      setPicking(null)
                    }}
                    onCancel={() => setPicking(null)}
                  />
                ) : null}
              </div>
              <div className="flex flex-col gap-1.5">
                <p className="text-xs text-[var(--color-muted)]">{strings.settings.store}</p>
                <Fact path={store ?? folder.data?.store ?? status.hostStorageRoot} unknown={strings.settings.storeUnknown} />
                <p className={HINT}>
                  {folder.data?.storeLockedBy
                    ? strings.settings.modelsFolderLocked(folder.data.storeLockedBy)
                    : strings.setup.storeHint}
                </p>
                {folder.data?.canChoose && folder.data.storeLockedBy === null ? (
                  <Choose
                    title={strings.folderPicker.storeTitle}
                    open={picking === STORE}
                    start={store ?? folder.data.store ?? chosen ?? folder.data.path}
                    code={code}
                    onOpen={() => setPicking(STORE)}
                    onChoose={(path) => {
                      setStore(path)
                      setPicking(null)
                    }}
                    onCancel={() => setPicking(null)}
                  />
                ) : null}
              </div>
            </section>
          ) : (
            <section className="mt-6 flex flex-col gap-4">
              <h3 className={SECTION_TITLE}>{strings.setup.optionsTitle}</h3>
              <label className="flex items-start gap-3 text-sm">
                <input
                  type="checkbox"
                  checked={importLinks}
                  disabled={status.importLinksForcedOff}
                  onChange={(event) => setImportLinks(event.target.checked)}
                  className="mt-0.5 size-4 accent-[var(--color-dim)]"
                />
                <span className="flex flex-col gap-1">
                  {strings.setup.importLinks}
                  <span className={HINT}>
                    {status.importLinksForcedOff ? strings.setup.importLinksForcedOff : strings.setup.importLinksHint}
                  </span>
                </span>
              </label>
              <label className="flex items-start gap-3 text-sm">
                <input
                  type="checkbox"
                  checked={wall}
                  onChange={(event) => setWall(event.target.checked)}
                  className="mt-0.5 size-4 accent-[var(--color-dim)]"
                />
                <span className="flex flex-col gap-1">
                  {strings.setup.wall}
                  <span className={HINT}>{strings.setup.wallHint}</span>
                </span>
              </label>
              <Field
                label={strings.setup.workers}
                hint={
                  running.data?.lockedBy
                    ? strings.settings.portLocked(running.data.lockedBy)
                    : strings.setup.workersHint
                }
              >
                <input
                  type="number"
                  min={MIN_WORKERS}
                  max={MAX_WORKERS}
                  value={workers ?? String(running.data?.workers ?? '')}
                  disabled={running.data === undefined || running.data.lockedBy !== null}
                  onChange={(event) => setWorkers(event.target.value)}
                  className={`${INPUT} tabular w-20`}
                />
              </Field>
              <p className={HINT}>{strings.setup.keysLater}</p>
              <p className={HINT}>{strings.setup.later}</p>
            </section>
          )}

          {step === 1 && (problem ?? otherRefusal) !== null ? (
            <p role="alert" className="mt-4 text-sm text-[var(--color-text)]">
              {problem ?? otherRefusal}
            </p>
          ) : null}
        </Step>
      </Presence>

      {finish.isError && !(finish.error instanceof Refused) ? (
        <p role="alert" className="mt-4 text-sm text-[var(--color-text)]">
          {strings.setup.failed}
        </p>
      ) : null}
      {finish.isPending || finish.isSuccess ? (
        // Kept while a moved port or a restart is followed: the page leaves when the server answers.
        <p role="status" className="mt-4 text-sm text-[var(--color-muted)]">
          {strings.setup.finishing}
        </p>
      ) : null}

      <div className="mt-8 flex items-center gap-3">
        <button
          type="submit"
          disabled={finish.isPending || finish.isSuccess}
          className={`${STANDING} whitespace-nowrap disabled:opacity-50`}
        >
          {step < STEPS ? strings.setup.next : strings.setup.finish}
        </button>
        {step > 1 ? (
          <button type="button" onClick={() => go(step - 1)} className={QUIET}>
            {strings.setup.back}
          </button>
        ) : null}
      </div>
    </form>
  )
}

/** "Choose…", and the folder picker it opens. */
function Choose({
  title,
  open,
  start,
  code,
  onOpen,
  onChoose,
  onCancel,
}: {
  title: string
  open: boolean
  start: string | null
  code: string
  onOpen: () => void
  onChoose: (path: string) => void
  onCancel: () => void
}) {
  return (
    <>
      <button type="button" onClick={onOpen} className={`${QUIET} self-start border border-[var(--color-edge)]`}>
        {strings.folderPicker.choose}
      </button>
      <Presence>
        {open ? (
          <FolderPicker key="picker" title={title} start={start} code={code} onChoose={onChoose} onCancel={onCancel} />
        ) : null}
      </Presence>
    </>
  )
}

const SETTINGS_PATH = '/settings'
/** Where a finished setup lands, wherever Lapidary went: `/setup` would only say it is set up. */
const HOME = '/'

/** The port this page is on, as a number, whatever the scheme's default. */
function thisPort(): number {
  if (window.location.port !== '') return Number(window.location.port)
  return window.location.protocol === 'https:' ? 443 : 80
}
