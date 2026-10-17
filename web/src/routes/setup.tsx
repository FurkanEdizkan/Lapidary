import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Doorway, Field, INPUT } from '../components/Doorway'
import { HEADLINE, LEAD, SECTION_TITLE, STANDING } from '../components/Page'
import { LOGIN_PATH, Refused, fetchSetupStatus, runSetup } from '../lib/session'
import { strings } from '../lib/strings'
import type { SetupStatus } from '../lib/types'

export const Route = createFileRoute('/setup')({
  component: SetupPage,
})

const STEPS = 3
const MIN_PASSWORD = 8

const QUIET =
  'ease-mechanical rounded-[var(--radius-ctl)] px-3 py-2 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]'

/**
 * The first-run wizard (A1): the admin's account, what this server reads and keeps, and the one option
 * worth deciding before anything is imported. Nothing is saved until Finish, so Back loses nothing;
 * a wrong setup code is the server's to say, and sends the person back to the step that has it.
 */
export function SetupPage() {
  const status = useQuery({ queryKey: ['setup-status'], queryFn: fetchSetupStatus })
  return (
    <Doorway>
      <title>{strings.titles.setup}</title>
      <h2 className={HEADLINE}>{strings.setup.title}</h2>
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
        <Wizard status={status.data} />
      )}
    </Doorway>
  )
}

function AlreadyDone() {
  return (
    <p className={LEAD}>
      <a href={LOGIN_PATH} className="underline underline-offset-2">
        {strings.login.title}
      </a>
    </p>
  )
}

function Wizard({ status }: { status: SetupStatus }) {
  const [step, setStep] = useState(1)
  const [code, setCode] = useState('')
  const [username, setUsername] = useState('')
  const [displayName, setDisplayName] = useState(status.suggestedName ?? '')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [importLinks, setImportLinks] = useState(!status.importLinksForcedOff)
  const [problem, setProblem] = useState<string | null>(null)

  const finish = useMutation({
    mutationFn: () =>
      runSetup({
        code,
        username,
        displayName: displayName.trim() === '' ? null : displayName,
        password,
        importLinks: status.importLinksForcedOff ? null : importLinks,
      }),
    onSuccess: () => window.location.assign('/'),
    onError: (error) => {
      // Every refusal the server can give is about the first step's fields.
      if (error instanceof Refused) setStep(1)
    },
  })

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
          if (wrong === null) setStep(2)
        } else if (step === 2) {
          setStep(3)
        } else if (!finish.isPending) {
          finish.mutate()
        }
      }}
    >
      <p className="text-xs text-[var(--color-dim)]">{strings.setup.step(step, STEPS)}</p>

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
              onChange={(event) => setCode(event.target.value)}
              className={`${INPUT} tabular tracking-wider uppercase`}
            />
          </Field>
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
          <dl className="flex flex-col gap-3 text-sm">
            <Fact label={strings.setup.modelsFolder} value={status.hostIngestRoot} />
            <Fact label={strings.setup.inbox} value={status.hostInboxRoot} />
            <Fact label={strings.setup.store} value={status.hostStorageRoot} />
          </dl>
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

      {problem !== null && step === 1 ? (
        <p role="alert" className="mt-4 text-sm text-[var(--color-text)]">
          {problem}
        </p>
      ) : null}
      {finish.isError ? (
        <p role="alert" className="mt-4 text-sm text-[var(--color-text)]">
          {finish.error instanceof Refused ? finish.error.message : strings.setup.failed}
        </p>
      ) : null}

      <div className="mt-8 flex items-center gap-3">
        <button type="submit" disabled={finish.isPending} className={`${STANDING} disabled:opacity-50`}>
          {step < STEPS ? strings.setup.next : finish.isPending ? strings.setup.finishing : strings.setup.finish}
        </button>
        {step > 1 ? (
          <button type="button" onClick={() => setStep(step - 1)} className={QUIET}>
            {strings.setup.back}
          </button>
        ) : null}
      </div>
    </form>
  )
}

function Fact({ label, value }: { label: string; value: string | null }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-xs text-[var(--color-muted)]">{label}</dt>
      <dd className={value === null ? 'text-[var(--color-dim)]' : 'font-mono text-[13px] break-all'}>
        {value ?? strings.setup.notSaid}
      </dd>
    </div>
  )
}
