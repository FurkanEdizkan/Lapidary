import { describe, it, expect } from 'vitest';
import { WORKER_HANDLERS } from '../src/worker.js';
import { indexArchiveJob } from '../src/services/indexArchive.service.js';
import { thumbnailJob } from '../src/services/thumbnail.service.js';

describe('worker entry handler map', () => {
  it('registers indexArchiveJob for index_archive', () => {
    expect(WORKER_HANDLERS.index_archive).toBe(indexArchiveJob);
  });

  it('registers thumbnailJob for thumbnail', () => {
    expect(WORKER_HANDLERS.thumbnail).toBe(thumbnailJob);
  });

  it('does not yet handle image_fetch', () => {
    expect(WORKER_HANDLERS.image_fetch).toBeUndefined();
  });
});
