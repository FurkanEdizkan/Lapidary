# Lapidary: accounts, one storage location, setup wizard, live port

## Context

The owner wants one install that several people can use, set up the way Jellyfin is (2026-10-17). Their answers so
far:

- **Per person.** Each person starts with an **empty library**. A model someone adds is visible only to them,
  unless they share it with chosen people or with everyone. Peer-to-peer ("torrent-like") sharing comes later,
  not now.
- **One storage location.** Models live in a single folder: the person's existing ones, plus uploads and link
  imports.
  - Lapidary may write new files there, and may **move** a removed model's file into a removed folder.
  - It never deletes a file. Removing a model is never a mistake that loses the file.
- **Adding models.** An **Import** button opens an in-app file browser over the storage location. A person adds
  a file or a folder.
  - Optionally they give a model link (MyMiniFactory, Printables, Thingiverse). With no link the model has no
    picture, and they can add pictures while adding.
  - With a link, and the site's API key saved, a **"Fetch details"** button pulls pictures, tags, licence and
    creator. Settings can make that automatic.
  - "Import from a creator link" (downloading the files) stays.
- **Accounts.** The admin creates accounts; there is no public sign-up. Sharing grants view and download only.
  The admin manages accounts but doesn't see other people's models.
- **The port.** It has a default, and if that's free the app takes it. Changing it in Settings moves the app
  there seamlessly. While anything loads, the pages show skeletons.

What exists today:
- No auth at all.
- A single-row `profile` table (0060), whose comment says accounts replace it.
- Global libraries, each with one `root` folder (0062).
- A read-only `/ingest` mount, plus a separate writable `/inbox` (M4).
- Ports fixed by compose's `ports:` mapping.

## Storage: one location, files are moved and never unlinked

- **One mount.** compose mounts `${LAPIDARY_MODELS_DIR}` at `/models` **read-write**, and it replaces both
  `/ingest` and `/inbox`.
  - Lapidary's own writes go under it: `uploads/<username>/…` and `imports/<site>/<creator>/<model>/…`.
  - Removed files go to `_removed/<username>/<original relative path>`. That's a `rename(2)` on the same
    filesystem, so it's atomic and never a copy followed by a delete.
  - The file browser and scans never show or index `_removed/`.
  - The app's own data (picture blobs, Postgres) stays in `/var/lib/lapidary`.
- **The storage handle.** `lapidary-storage` gains one `ModelStore` handle in place of `LibraryReader` and
  `InboxWriter`. It has `read`, `put_new` (refuses to overwrite), `list_dir` and `move_to_removed`, and **no
  unlink method at all**.
  - A test asserts the crate has no `remove_file`/`remove_dir` call outside `DerivativeStore`, extending the
    existing `check-deploy` guard on `InboxWriter`.
- **What removing a model does.**
  - The rows go: the part, its pictures, tags and shares.
  - The file moves to `_removed/` **only if no other part, of anyone, still points at it**. Otherwise only the
    rows go, and the confirm sentence says which of the two will happen.
  - A person can get a file back by moving it out of `_removed/` and adding it again. `ponytail:` there's no
    in-app restore; add a Removed list if it's asked for.
- **Removing a whole added folder** stops indexing it and removes its parts. It moves **no** files, because the
  folder stays the owner's, and the dialog says so.
- **Migration.** An existing install's files stay where they are:
  - `/ingest` becomes `/models`;
  - `inbox` contents move under `/models` once, through a documented one-line `mv` in the upgrade notes, rather
    than through app code.
  - `file.root` collapses to `models` (and `store` for pre-M4 rows).

## Accounts and visibility

- **`app_user`:**
  - Columns: `id`, `username` (unique, case-folded), `display_name`, `password_hash` (argon2id), `is_admin`,
    `created_at`.
  - It replaces `profile`, whose display name is copied into the first admin.
  - Creating a user also creates that user's library: one per person, named after them, and empty.
- **`session`:**
  - Columns: `token_hash` (BLAKE3 of 32 random bytes), `user_id`, `expires_at` (30 days, sliding).
  - The cookie `lapidary_session` is HttpOnly and SameSite=Strict. It is Secure when
    `LAPIDARY_COOKIE_SECURE=true`.
  - Mutating requests must carry an `Origin` that matches `Host`.
- **Library and model sources.**
  - `library.owner_id`.
  - `library_source(library_id, path, kind file|folder, page_url null)` replaces `library.root`. That's what
    the Import browser adds, and what a scan walks.
  - `part.owner_id` is the library's owner.
  - Two people adding the same folder each get their own parts.
- **Sharing.** `part_share(part_id, user_id)` for chosen people, plus `part.shared_with_everyone`.
- **Who sees what.**
  - A part is visible to a user when it's theirs, shared with everyone, or shared with them. It's one predicate
    macro `visible_to!` spliced into the existing `grid_columns!`/`grid_laterals!` queries
    (`crates/lapidary-db/src/repo.rs:1334`).
  - Writes check `owner_id = me`.
  - A part someone can't see is a 404, indistinguishable from an unknown id.
  - That includes `blob.rs` reachability, because content addressing is not authorization.
- **Shared with me** is a place in `AppFrame` beside Parts and Dashboard.
- **Site keys are per user:** `site_key` gains `user_id`.
- **Deleting a user** moves their library and parts to the admin who deletes them, stated in the confirm
  sentence.

## Import: browse, add, describe

- **The routes.**
  - `GET /api/storage/browse?path=` lists a directory under `/models`: directories, then model files, with sizes.
    It skips `_removed/` and refuses `..` and symlinks that escape, reusing `derive::create_library`'s root
    validation.
  - `POST /api/library/sources {path, kind, pageUrl?, pictures?}` adds a source to *my* library and queues the
    existing `ingest` job for it, index in place and hash first.
- **The Import dialog (`web/src/components/ImportDialog.tsx`)** replaces "Upload a folder" in the top bar. It
  has three tabs:
  1. **Browse:** a folder and file list with breadcrumbs. A person ticks files or folders, and for each pick can
     set a **model link** and attach **pictures**.
  2. **Upload:** today's upload, now written to `uploads/<username>/`.
  3. **From a link:** today's `ImportLink.tsx`, downloading into `imports/`.
- **"Fetch details".**
  - Given a page URL and a saved key, `POST /api/parts/{id}/describe-from-page` runs the page half of
    `link.rs::describe_from_page`: source row, tags, and every picture through `add_page_image` (M5). It never
    downloads files.
  - Settings has "Fetch details automatically when a link is given" (`user_setting`, default off), which runs
    the same thing at the end of the ingest job.
  - Without a key the button names the missing key and links to Settings.

## Setup wizard and sign-in

- **No users yet.** Every `/api` route except `/api/healthz` and `/api/setup*` answers 409 `setup_required`,
  and the web goes to `/setup`.
- **Setup code.** At first start the server logs the URL and a one-time **setup code**, and wizard step 1 asks
  for it. This keeps a stranger on the LAN from claiming admin. `LAPIDARY_SETUP_CODE` overrides it.
- **The wizard (`routes/setup.tsx`):**
  1. Setup code, admin username, display name, password.
  2. **Server:**
     - the port, editable and prefilled with the one the app took;
     - the storage location as a read-only fact, with the `deploy/.env` line that changes it;
     - an optional check that `/models` is writable.
  3. **Options:**
     - import-from-link on or off (`instance_setting`; the env var still forces it off);
     - fetch details automatically;
     - site keys (reuses the `settings.tsx` form).
  4. Done. The admin lands in their empty library with the Import dialog open.
- **Creating the admin.** `POST /api/setup` runs in one transaction guarded by `NOT EXISTS (SELECT 1 FROM
  app_user)`. In the same transaction it claims every owner-less library and part, which is how today's six
  parts become the admin's.
- **Sign-in.**
  - `/login`; `POST`/`DELETE`/`GET /api/session`.
  - A wrong password costs 1 s. `ponytail:` no lockout counter.
  - `docker compose exec app lapidary-server reset-password <user>` is the escape hatch for a lost admin
    password.

## Port: chosen at first start, moved live from Settings

- **compose.** The `app` service moves to **`network_mode: host`** and binds the port itself; a container
  can't change its own `ports:` mapping.
  - `db` is published only on `127.0.0.1:${LAPIDARY_DB_PORT:-5439}`.
  - `check-deploy` learns the new shape.
- **First start.** The app tries `LAPIDARY_PORT` (default 3000), then 3001–3020, and logs the URL it took. The
  value is saved in `instance_setting.port`.
- **What wins.** If `LAPIDARY_PORT` is set **explicitly** in `deploy/.env`, it wins, and the field is locked:
  "Set by LAPIDARY_PORT in deploy/.env".
- **Changing it (`PUT /api/settings/port`, admin only).**
  1. The server binds the new port **beside** the old one, serving the same Router. A busy port is refused with
     a sentence: "Port 3005 is in use by another program — pick another."
  2. It saves the setting and answers `{url}`.
  3. The page shows the app skeleton while it polls `<url>/api/healthz`, then navigates there. Cookies ignore
     ports, so the person stays signed in.
  4. The old listener serves for 60 s, then shuts down gracefully.
  - The listeners are owned by `bin/lapidary-server` (a `Listeners` handle passed in through an `Extension`).
    `lapidary-api` never owns a socket.
- **Skeletons** replace the "Loading…" text in the grid (cards in the current layout and density), the part page
  and the dashboard. They pulse in opacity, are static under `prefers-reduced-motion`, and are the same skeleton
  the port move shows.

## Code shape

- **API.** `lapidary-api` gains `auth.rs`: a `require_user` layer inside `router()` that puts
  `CurrentUser {id, is_admin}` in request extensions. There's no new `AppState` field; that struct is built in
  about 88 places.
- **Repository.** Repository calls that read or write parts take `viewer: UserId`.
- **Events.** SSE events go only to the owner of the library they name.
- **One new dependency: `argon2`**, pinned. Randomness comes from `getrandom`, which is already in the lock.
- **Web.**
  - One `api()` wrapper in `lib/api.ts` over its ~53 `fetch` calls: a 401 goes to `/login`, and a 409 setup
    goes to `/setup`.
  - `UserMenu` shows Profile, Settings, Users (admin only) and Sign out.
  - The share dialog works on one part or the grid selection, with three choices: everyone, chosen people, or
    nobody.
  - Every string goes through `strings.ts`.
- **CLAUDE.md, as rewritten by the lead:**
  - "**One storage location; files are moved, never deleted.** Lapidary writes only new files (uploads, imports)
    and moves a removed model's file into `_removed/`, never over another part's file and never with an
    unlink." This replaces "Your folders are read where they are".
  - "Delete is delete" becomes **"Remove is remove"**: rows go at once, there's no restore in the app, and the
    file goes to `_removed/`.
  - New: "**A person sees their own models and the ones shared with them**; anything else is a 404."

## Goals (lead adds them to `docs/goals/BOARD.md`), in order

1. **A1, accounts and setup:**
   - migration `0063_accounts.sql`: `app_user`, `session`, `instance_setting`, `user_setting`, nullable owner
     columns, a library per user;
   - `auth.rs`, the setup and session routes, the setup code, `reset-password`;
   - the `/setup` and `/login` pages and the `api()` wrapper;
   - a shared `tests/common::signed_in(pool)` that every `crates/lapidary-api/tests/*.rs` `send` helper uses.
2. **A2, one storage location:**
   - migration `0064`: `library_source` replaces `root`, and `file.root` becomes `models`;
   - the `ModelStore` handle (no unlink) and the compose mount;
   - remove moves the file to `_removed/`;
   - the browse and sources routes, and the Import dialog's Browse tab with link and pictures;
   - Fetch details, and the setting that makes it automatic.
3. **A3, models per person:**
   - owner columns become NOT NULL;
   - `viewer` through every repository read and write;
   - `blob.rs`, downloads, events, the job owner, per-user `site_key`;
   - a cross-user isolation test over every route group in `lib.rs`.
4. **A4, sharing and accounts admin:** `part_share`, `shared_with_everyone`, the share dialog, Shared with me, the
   users page, password change and reset, delete with transfer.
5. **A5, port and skeletons.** This can run beside A3.
   - host networking, free-port pick, the live move;
   - the Settings "Server" section;
   - skeletons.

**Left for later, stated in FEATURES:**
- F2's copy-link now needs a signed-in browser; a per-link download token is the follow-up.
- Peer-to-peer shared libraries.
- An in-app Removed list.

## Verification

- `cargo xtask verify slice` passes in the foreground for each goal.
- **Setup:**
  - a fresh rig answers 409 `setup_required`, and `/` lands on `/setup`;
  - a wrong setup code is refused;
  - the wizard makes the admin, and that admin has an empty library;
  - a second `POST /api/setup` gets 409.
- **Storage:**
  - Adding a folder through Browse indexes it in place.
  - Removing one model moves its file to `_removed/<user>/…`, and `ls` shows it there, not gone.
  - Removing a model whose file another user also has moves nothing.
  - `strace -f -e trace=unlink,unlinkat` on the app during a remove shows no unlink under `/models`.
- **Fetch details:** a real Printables URL on an added local file gives its pictures, tags and licence, and no
  file is downloaded.
- **Isolation:**
  - user B's grid, search, dashboard, `/api/blob/{A's picture}` and `/api/parts/{A's part}/file` are empty or
    404;
  - after A shares one part, B sees exactly that part under Shared with me, can download it, and gets 403 on
    edit.
- **Port:**
  - with 31000 busy the rig comes up on 31001;
  - Settings moves it to 31005 behind the skeleton, still signed in;
  - the old port answers for 60 s and is then gone;
  - a busy port is refused with its sentence;
  - a set `LAPIDARY_PORT` locks the field.
- **Upgrade:** the owner's install on 3000 goes through setup once, the admin owns the six existing parts, and
  headless Chrome screenshots of the wizard, the login page, the Import dialog and the skeleton are checked.
