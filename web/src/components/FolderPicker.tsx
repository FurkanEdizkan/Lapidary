import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { fetchHostFolders } from "../lib/api";
import { strings } from "../lib/strings";

const BUTTON =
  "ease-mechanical rounded-[var(--radius-ctl)] border border-[var(--color-edge)] px-3 py-1.5 text-sm duration-[var(--duration-fast)] hover:-translate-y-px disabled:opacity-50";
const QUIET =
  "ease-mechanical rounded-[var(--radius-ctl)] px-3 py-1.5 text-sm text-[var(--color-muted)] duration-[var(--duration-fast)] hover:text-[var(--color-text)] disabled:opacity-50";

/**
 * The models-folder picker (A6): one folder of the machine at a time, opened by name, and chosen with
 * "Use this folder" when Lapidary may write there. A folder it can't open or write says what to run.
 * `code` is the setup code in the wizard, before anybody can sign in.
 */
export function FolderPicker({
  start,
  code = null,
  onChoose,
  onCancel,
}: {
  start: string | null;
  code?: string | null;
  onChoose: (path: string) => void;
  onCancel: () => void;
}) {
  const [at, setAt] = useState<string | null>(start);
  const listing = useQuery({
    queryKey: ["host-folders", at, code],
    queryFn: () => fetchHostFolders(at, code),
  });
  const data = listing.data;
  return (
    <div className="flex flex-col gap-2 rounded-[var(--radius-ctl)] border border-[var(--color-edge)] p-3">
      <p className="font-mono text-[13px] break-all">
        {data?.path ?? at ?? ""}
      </p>
      {listing.isPending ? (
        <p className="text-sm text-[var(--color-muted)]">
          {strings.folderPicker.loading}
        </p>
      ) : listing.isError ? (
        <p role="alert" className="text-sm text-[var(--color-text)]">
          {listing.error.message || strings.folderPicker.failed}
        </p>
      ) : (
        <>
          {data?.problem ? (
            <p
              role="status"
              className="text-xs text-[var(--color-muted)] break-words"
            >
              {data.problem}
            </p>
          ) : null}
          {data?.folders.length === 0 && !data.problem ? (
            <p className="text-sm text-[var(--color-dim)]">
              {strings.folderPicker.empty}
            </p>
          ) : null}
          <ul
            role="list"
            aria-label={strings.folderPicker.listLabel}
            className="flex max-h-64 flex-col overflow-y-auto"
          >
            {data?.folders.map((name) => (
              <li key={name}>
                <button
                  type="button"
                  aria-label={strings.folderPicker.open(name)}
                  onClick={() =>
                    setAt(`${data.path.replace(/\/$/, "")}/${name}`)
                  }
                  className="w-full truncate rounded-[var(--radius-ctl)] px-2 py-1 text-left text-sm hover:bg-[var(--color-raised)]"
                >
                  {name}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      <div className="flex flex-wrap items-center gap-2 pt-1">
        <button
          type="button"
          disabled={!data?.choosable}
          onClick={() => data && onChoose(data.path)}
          className={BUTTON}
        >
          {strings.folderPicker.use}
        </button>
        <button
          type="button"
          disabled={!data?.parent}
          onClick={() => data?.parent && setAt(data.parent)}
          className={QUIET}
        >
          {strings.folderPicker.up}
        </button>
        <button type="button" onClick={onCancel} className={QUIET}>
          {strings.folderPicker.cancel}
        </button>
      </div>
    </div>
  );
}
