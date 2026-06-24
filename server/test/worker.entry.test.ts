import { describe, it, expect } from 'vitest';
import { WORKER_HANDLERS } from '../src/worker.js';
import { indexArchiveJob } from '../src/services/indexArchive.service.js';

describe('worker entry handler map', () => {
  it('registers indexArchiveJob for index_archive', () => {
    expect(WORKER_HANDLERS.index_archive).toBe(indexArchiveJob);
  });

  it('does not yet handle thumbnail or image_fetch', () => {
    expect(WORKER_HANDLERS.thumbnail).toBeUndefined();
    expect(WORKER_HANDLERS.image_fetch).toBeUndefined();
  });
});
