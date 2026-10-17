import { createFileRoute } from "@tanstack/react-router";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Doorway, Field, INPUT } from "../components/Doorway";
import { HEADLINE, LEAD, SECTION_TITLE, STANDING } from "../components/Page";
import {
  LOGIN_PATH,
  Refused,
  fetchSetupStatus,
  runSetup,
} from "../lib/session";
import {
  chooseModelsFolder,
  fetchModelsFolder,
  followPort,
  followRestart,
  movePort,
} from "../lib/api";
import { FolderPicker } from "../components/FolderPicker";
import { strings } from "../lib/strings";
import type { SetupStatus } from "../lib/types";

export const Route = createFileRoute("/setup")({
  component: SetupPage,
});

const STEPS = 3;
const MIN_PASSWORD = 8;

const QUIET =
  "ease-mechanical rounded-[var(--radius-ctl)] px-3 py-2 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)]";

/**
 * The first-run wizard (A1): the admin's account, what this server reads and keeps, and the one option
 * worth deciding before anything is imported. Nothing is saved until Finish, so Back loses nothing;
 * a wrong setup code is the server's to say, and sends the person back to the step that has it.
 */
export function SetupPage() {
  const status = useQuery({
    queryKey: ["setup-status"],
    queryFn: fetchSetupStatus,
  });
  return (
    <Doorway>
      <title>{strings.titles.setup}</title>
      <h2 className={HEADLINE}>{strings.setup.title}</h2>
      {status.isPending ? (
        <p className="mt-6 text-[var(--color-muted)]">
          {strings.setup.loading}
        </p>
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
  );
}

function AlreadyDone() {
  return (
    <p className={LEAD}>
      <a href={LOGIN_PATH} className="underline underline-offset-2">
        {strings.login.title}
      </a>
    </p>
  );
}

function Wizard({ status }: { status: SetupStatus }) {
  const [step, setStep] = useState(1);
  const [code, setCode] = useState("");
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState(status.suggestedName ?? "");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [importLinks, setImportLinks] = useState(!status.importLinksForcedOff);
  // The port this page was opened on (A5); changing it moves Lapidary there once the account exists.
  const here = window.location.port === "" ? "" : window.location.port;
  const [port, setPort] = useState(here);
  // The models folder (A6), chosen here and applied once the account exists: Lapidary restarts on it.
  const folder = useQuery({
    queryKey: ["setup-models-folder", code],
    queryFn: () => fetchModelsFolder(code),
    enabled: step === 2,
    retry: false,
  });
  const [chosen, setChosen] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const finish = useMutation({
    mutationFn: () =>
      runSetup({
        code,
        username,
        displayName: displayName.trim() === "" ? null : displayName,
        password,
        importLinks: status.importLinksForcedOff ? null : importLinks,
      }),
    onSuccess: async () => {
      const to = Number(port);
      let moved = false;
      if (port !== here && Number.isInteger(to) && to >= 1024) {
        // Signed in now, so the move is allowed; a refusal (a fixed or busy port) leaves Lapidary
        // where it is, and the person can move it from Settings.
        try {
          await movePort(to);
          moved = true;
        } catch {
          // Stay on this port.
        }
      }
      if (chosen !== null && chosen !== folder.data?.path) {
        try {
          // The restart comes back on the saved port, moved or not.
          const restarting = await chooseModelsFolder(chosen);
          if (restarting && (await followRestart(moved ? to : thisPort())))
            return;
        } catch {
          // Refused: Settings shows the folder and why.
          window.location.assign(SETTINGS_PATH);
          return;
        }
      }
      if (moved && (await followPort(to))) return;
      window.location.assign("/");
    },
    onError: (error) => {
      // Every refusal the server can give is about the first step's fields.
      if (error instanceof Refused) setStep(1);
    },
  });

  const accountReady = () => {
    if (code.trim() === "" || username.trim() === "" || password === "")
      return strings.setup.missing;
    if (password.length < MIN_PASSWORD) return strings.setup.passwordShort;
    if (password !== confirm) return strings.setup.passwordsDiffer;
    return null;
  };

  return (
    <form
      className="mt-2 flex flex-col"
      onSubmit={(event) => {
        event.preventDefault();
        if (step === 1) {
          const wrong = accountReady();
          setProblem(wrong);
          if (wrong === null) setStep(2);
        } else if (step === 2) {
          setStep(3);
        } else if (!finish.isPending) {
          finish.mutate();
        }
      }}
    >
      <p className="text-xs text-[var(--color-dim)]">
        {strings.setup.step(step, STEPS)}
      </p>

      {step === 1 ? (
        <section className="mt-6 flex flex-col gap-4">
          <h3 className={SECTION_TITLE}>{strings.setup.accountTitle}</h3>
          <p className="-mt-2 text-sm text-[var(--color-dim)]">
            {strings.setup.accountLead}
          </p>
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
          <Field
            label={strings.setup.usernameLabel}
            hint={strings.setup.usernameHint}
          >
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
          <Field
            label={strings.setup.passwordLabel}
            hint={strings.setup.passwordHint}
          >
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
          <p className="-mt-2 text-sm text-[var(--color-dim)]">
            {strings.setup.serverLead}
          </p>
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
            <p className="text-xs text-[var(--color-muted)]">
              {strings.setup.modelsFolder}
            </p>
            <p className="font-mono text-[13px] break-all">
              {chosen ??
                folder.data?.path ??
                status.hostIngestRoot ??
                strings.settings.modelsFolderUnknown}
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
            {folder.data?.canChoose ? (
              picking ? (
                <FolderPicker
                  start={chosen ?? folder.data.path}
                  code={code}
                  onChoose={(path) => {
                    setChosen(path);
                    setPicking(false);
                  }}
                  onCancel={() => setPicking(false)}
                />
              ) : (
                <button
                  type="button"
                  onClick={() => setPicking(true)}
                  className={`${QUIET} self-start border border-[var(--color-edge)]`}
                >
                  {strings.folderPicker.choose}
                </button>
              )
            ) : null}
          </div>
          <dl className="flex flex-col gap-3 text-sm">
            <Fact
              label={strings.settings.store}
              value={
                folder.data?.store ??
                status.hostStorageRoot ??
                strings.settings.storeUnknown
              }
            />
          </dl>
          <p className="-mt-2 text-xs text-[var(--color-dim)]">
            {strings.settings.storeHint}
          </p>
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
                {status.importLinksForcedOff
                  ? strings.setup.importLinksForcedOff
                  : strings.setup.importLinksHint}
              </span>
            </span>
          </label>
          <p className="text-xs text-[var(--color-dim)]">
            {strings.setup.keysLater}
          </p>
        </section>
      )}

      {problem !== null && step === 1 ? (
        <p role="alert" className="mt-4 text-sm text-[var(--color-text)]">
          {problem}
        </p>
      ) : null}
      {finish.isError ? (
        <p role="alert" className="mt-4 text-sm text-[var(--color-text)]">
          {finish.error instanceof Refused
            ? finish.error.message
            : strings.setup.failed}
        </p>
      ) : null}

      <div className="mt-8 flex items-center gap-3">
        <button
          type="submit"
          disabled={finish.isPending}
          className={`${STANDING} disabled:opacity-50`}
        >
          {step < STEPS
            ? strings.setup.next
            : finish.isPending
              ? strings.setup.finishing
              : strings.setup.finish}
        </button>
        {step > 1 ? (
          <button
            type="button"
            onClick={() => setStep(step - 1)}
            className={QUIET}
          >
            {strings.setup.back}
          </button>
        ) : null}
      </div>
    </form>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-xs text-[var(--color-muted)]">{label}</dt>
      <dd className="font-mono text-[13px] break-all">{value}</dd>
    </div>
  );
}

const SETTINGS_PATH = "/settings";

/** The port this page is on, as a number, whatever the scheme's default. */
function thisPort(): number {
  if (window.location.port !== "") return Number(window.location.port);
  return window.location.protocol === "https:" ? 443 : 80;
}
