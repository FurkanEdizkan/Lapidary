import { LRUCache } from 'lru-cache';
import { config } from '../config.js';

/**
 * A tiny cache abstraction backed by Redis when REDIS_URL is set and reachable,
 * otherwise an in-process LRU. Every read/write degrades gracefully: a Redis error
 * never throws to callers — it falls back to (or simply skips) the local cache.
 *
 * Invalidation is namespace-versioned: bumping the version makes every key in a
 * namespace stale without scanning/deleting individual keys.
 */
interface RedisLike {
  get(key: string): Promise<string | null>;
  set(key: string, val: string, mode: string, ttl: number): Promise<unknown>;
  incr(key: string): Promise<number>;
  on(event: string, cb: (...args: unknown[]) => void): unknown;
}

const lru = new LRUCache<string, string>({ max: 2000, ttl: 1000 * 60 * 10 });
const localVersions = new Map<string, number>();

let redis: RedisLike | null = null;
let redisHealthy = false;

export async function initCache(): Promise<void> {
  if (!config.redisUrl) return;
  try {
    const { default: Redis } = await import('ioredis');
    const client = new Redis(config.redisUrl, {
      lazyConnect: true,
      maxRetriesPerRequest: 1,
      retryStrategy: () => null,
    }) as unknown as RedisLike;
    client.on('error', () => { redisHealthy = false; });
    client.on('ready', () => { redisHealthy = true; });
    await (client as unknown as { connect(): Promise<void> }).connect();
    redis = client;
    redisHealthy = true;
  } catch {
    redis = null;
    redisHealthy = false;
  }
}

export function cacheBackend(): 'redis' | 'lru' {
  return redis && redisHealthy ? 'redis' : 'lru';
}

async function versionOf(namespace: string): Promise<number> {
  if (redis && redisHealthy) {
    try {
      const v = await redis.get(`ver:${namespace}`);
      return v ? Number(v) : 1;
    } catch {
      redisHealthy = false;
    }
  }
  return localVersions.get(namespace) ?? 1;
}

/** Invalidate an entire namespace by bumping its version counter. */
export async function bumpNamespace(namespace: string): Promise<void> {
  if (redis && redisHealthy) {
    try {
      await redis.incr(`ver:${namespace}`);
      return;
    } catch {
      redisHealthy = false;
    }
  }
  localVersions.set(namespace, (localVersions.get(namespace) ?? 1) + 1);
}

/**
 * Get a cached value for (namespace, key), or compute + store it. `ttlSeconds`
 * bounds freshness; the namespace version prefixes the physical key.
 */
export async function cached<T>(
  namespace: string,
  key: string,
  ttlSeconds: number,
  compute: () => Promise<T> | T,
): Promise<T> {
  const version = await versionOf(namespace);
  const physical = `${namespace}:v${version}:${key}`;

  if (redis && redisHealthy) {
    try {
      const hit = await redis.get(physical);
      if (hit != null) return JSON.parse(hit) as T;
    } catch {
      redisHealthy = false;
    }
  } else {
    const hit = lru.get(physical);
    if (hit != null) return JSON.parse(hit) as T;
  }

  const value = await compute();
  const serialized = JSON.stringify(value);
  if (redis && redisHealthy) {
    try {
      await redis.set(physical, serialized, 'EX', ttlSeconds);
    } catch {
      redisHealthy = false;
      lru.set(physical, serialized);
    }
  } else {
    lru.set(physical, serialized);
  }
  return value;
}
