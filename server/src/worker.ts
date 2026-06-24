import { getDb } from './db/database.js';
import { startWorker, type HandlerMap } from './services/worker.service.js';
import { indexArchiveJob } from './services/indexArchive.service.js';

/** Job kinds this worker can process. Phase 2/3 add thumbnail + image_fetch. */
export const WORKER_HANDLERS: HandlerMap = {
  index_archive: indexArchiveJob,
};

/** Only start the loop when run as a process, not when imported by a test. */
const isMain = import.meta.url === `file://${process.argv[1]}`;
if (isMain) {
  const db = getDb(); // ensure migrations have run before polling
  const requeued = db
    .prepare("UPDATE jobs SET status = 'queued', updated_at = ? WHERE status = 'running'")
    .run(new Date().toISOString());
  if (requeued.changes > 0) console.log(`[worker] requeued ${requeued.changes} stale running job(s)`);
  const stop = startWorker(WORKER_HANDLERS, 1500);
  // eslint-disable-next-line no-console
  console.log('[worker] started; handling: index_archive');
  for (const sig of ['SIGINT', 'SIGTERM'] as const) {
    process.on(sig, () => {
      stop();
      process.exit(0);
    });
  }
}
