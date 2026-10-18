import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Suspense, lazy, useEffect, useRef, useState, type ReactNode } from 'react'
import { BrandMark } from '../components/AppFrame'
import { Doorway, Field, INPUT } from '../components/Doorway'
import { Icon } from '../components/Icon'
import { HEADLINE, LEAD, SECTION_TITLE, STANDING } from '../components/Page'
import { LOGIN_PATH, Refused, fetchSetupStatus, runSetup } from '../lib/session'
import { chooseModelsFolder, fetchModelsFolder, followPort, followRestart, movePort } from '../lib/api'
import { FolderPicker } from '../components/FolderPicker'
import { Fade, Indicator, Presence, Rise, SETTLE, Step } from '../lib/motion-react'
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
/** The refusal that is about the setup code itself, shown under that field. */
const WRONG_CODE = 'wrong_code'

const QUIET =
  'ease-mechanical rounded-[var(--radius-ctl)] px-3 py-2 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]'

/**
 * Lazy, so anime.js and the drawing load with the doorway and never with the grid (A19). A function as well, so the
 * intro's clock can wait for it: the intro starts when there is something to draw.
 */
const loadLattice = () => import('../components/Lattice')
const Lattice = lazy(loadLattice)

/**
 * The intro plays once per page load (DESIGN.md § Stepper). Set when it ends rather than when it starts, so a mount
 * that is thrown away (StrictMode's first) does not use it up.
 */
let introPlayed = false

/** At the centre drawing; travelling to the left column while the wizard rises; over. */
type Phase = 'centre' | 'travel' | 'end'

/** A skipped intro lands where it was going without travelling. */
const INSTANT = { duration: 0 }

/** Where the wizard is, and which way it last moved, so Back plays the step change in reverse. */
type At = { step: number; direction: 1 | -1 }

/**
 * The first-run wizard (A1): the admin's account, what this server reads and keeps, and the one option
 * worth deciding before anything is imported. Nothing is saved until Finish, so Back loses nothing;
 * a wrong setup code is the server's to say, and sends the person back to the step that has it.
 *
 * A20 puts it in the Doorway's split, after a short intro: the Lattice draws at the centre of the screen, then moves
 * into the left column as the wizard rises. A key, a click or Skip ends it at its end state; under reduced motion it
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
  const heading = useRef<HTMLHeadingElement>(null)

  // The clock: centre for `--duration-scene`, then the travel, all inside `--duration-intro`.
  useEffect(() => {
    if (!intro) return
    let live = true
    const timers: number[] = []
    void loadLattice().then(() => {
      if (!live) return
      const t = tokens()
      timers.push(window.setTimeout(() => setPhase((now) => (now === 'centre' ? 'travel' : now)), t.scene))
      timers.push(window.setTimeout(() => setPhase('end'), t.intro))
    })
    return () => {
      live = false
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

  // Ended: Skip has gone from under the focus, so the page's heading takes it.
  useEffect(() => {
    if (!intro || !over) return
    introPlayed = true
    heading.current?.focus({ preventScroll: true })
  }, [intro, over])

  const centre = phase === 'centre'
  const wizard = status.data?.required === true
  // What arrives after the intro rises in; without one (or skipped) it is simply there.
  const arrive = (children: ReactNode) => <Rise {...(intro && !skipped ? {} : { initial: false })}>{children}</Rise>

  return (
    <Doorway
      done={done}
      aside={
        <div className="flex w-full max-w-md flex-col gap-12">
          {/* At the centre of the screen during the intro, in the column after: one element, so it never redraws. */}
          <div
            className={
              centre
                ? 'pointer-events-none fixed inset-0 z-50 flex flex-col items-center justify-center gap-10 p-8'
                : 'contents'
            }
          >
            <Indicator
              id="setup-lattice"
              transition={skipped ? INSTANT : SETTLE}
              className={centre ? 'w-[min(26rem,76vw)]' : 'w-full'}
            >
              <Suspense fallback={null}>
                <Lattice className="w-full" still={!intro || skipped || done} total={tokens().intro} />
              </Suspense>
            </Indicator>
            {centre ? (
              <Fade>
                <BrandMark />
              </Fade>
            ) : null}
          </div>
          {wizard && !centre ? arrive(<Stepper step={at.step} />) : null}
        </div>
      }
    >
      <title>{strings.titles.setup}</title>
      <Presence>
        {centre ? (
          <Fade key="intro" className="fixed inset-0 z-40 bg-[var(--color-bg)]">
            <button
              type="button"
              onClick={() => {
                setSkipped(true)
                setPhase('end')
              }}
              className={`${QUIET} absolute right-4 bottom-4 border border-[var(--color-edge)] lg:right-8 lg:bottom-8`}
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
              <h2 ref={heading} tabIndex={-1} className={`${HEADLINE} outline-none`}>
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
              <Indicator id="setup-rail" className="absolute inset-y-0 left-[-0.5px] w-[2px] bg-[var(--color-edge)]" />
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
    onMutate: () => onFinishing(true),
    mutationFn: () =>
      runSetup({
        code,
        username,
        displayName: displayName.trim() === '' ? null : displayName,
        password,
        importLinks: status.importLinksForcedOff ? null : importLinks,
      }),
    onSuccess: async () => {
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
      const path = chosen !== null && chosen !== folder.data?.path ? chosen : null
      const kept = store !== null && store !== folder.data?.store ? store : null
      if (path !== null || kept !== null) {
        try {
          // The restart comes back on the saved port, moved or not.
          const restarting = await chooseModelsFolder({ path, store: kept })
          if (restarting && (await followRestart(moved ? to : thisPort()))) return
        } catch {
          // Refused: Settings shows the folder and why.
          window.location.assign(SETTINGS_PATH)
          return
        }
      }
      if (moved && (await followPort(to))) return
      window.location.assign('/')
    },
    onError: (error) => {
      onFinishing(false)
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
          <div className="h-px bg-[var(--color-edge)]" style={{ width: `${(step / STEPS) * 100}%` }} />
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
                <p className="font-mono text-[13px] break-all">
                  {chosen ?? folder.data?.path ?? status.hostIngestRoot ?? strings.settings.modelsFolderUnknown}
                </p>
                <p className="text-xs text-[var(--color-dim)]">
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
                <p className="font-mono text-[13px] break-all">
                  {store ?? folder.data?.store ?? status.hostStorageRoot ?? strings.settings.storeUnknown}
                </p>
                <p className="text-xs text-[var(--color-dim)]">
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
                  className="mt-0.5 size-4 accent-[var(--color-accent)]"
                />
                <span className="flex flex-col gap-1">
                  {strings.setup.importLinks}
                  <span className="text-xs text-[var(--color-dim)]">
                    {status.importLinksForcedOff ? strings.setup.importLinksForcedOff : strings.setup.importLinksHint}
                  </span>
                </span>
              </label>
              <p className="text-xs text-[var(--color-dim)]">{strings.setup.keysLater}</p>
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

/** The port this page is on, as a number, whatever the scheme's default. */
function thisPort(): number {
  if (window.location.port !== '') return Number(window.location.port)
  return window.location.protocol === 'https:' ? 443 : 80
}
