/** Typed fetch wrappers + React Query hooks for the Manifold API. */
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';

export interface Model {
  id: string;
  name: string;
  creator: string;
  type: string;
  meshKind: string | null;
  color: string;
  format: string;
  fileSizeBytes: number;
  size: [number, number, number];
  triangleCount: number;
  created: string | null;
  added: string;
  tags: string[];
  groups: string[];
  printers: string[];
  hasThumbnail: boolean;
  hasLod: boolean;
  hasOriginal: boolean;
}
export interface SettingRow { id: number; k: string; v: string; source: string; }
export interface ImageItem { id: number; url: string; caption: string | null; kind: string; }
export interface ModelDetail extends Model { notes: string | null; settings: SettingRow[]; images: ImageItem[]; }
export interface TagInfo { name: string; count: number; }
export interface GroupInfo { name: string; shared: boolean; count: number; }
export interface Pin { kind: 'creator' | 'tag' | 'group'; name: string; }
export interface Suggestions {
  models: { id: string; name: string; creator: string; type: string }[];
  creators: { name: string; count: number }[];
  tags: { name: string; count: number }[];
  tagHeader: string;
}

async function jget<T>(url: string): Promise<T> {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${r.status} ${url}`);
  return r.json() as Promise<T>;
}
async function jsend<T>(url: string, method: string, body?: unknown): Promise<T> {
  const r = await fetch(url, {
    method,
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  if (!r.ok) throw new Error(`${r.status} ${url}`);
  return r.json() as Promise<T>;
}

export interface ModelQuery { search?: string; tags?: string[]; group?: string | null; creator?: string | null; }

function modelsUrl(q: ModelQuery): string {
  const p = new URLSearchParams();
  if (q.search) p.set('search', q.search);
  if (q.tags?.length) p.set('tags', q.tags.join(','));
  if (q.group) p.set('group', q.group);
  if (q.creator) p.set('creator', q.creator);
  const s = p.toString();
  return `/api/models${s ? `?${s}` : ''}`;
}

export function useModels(q: ModelQuery) {
  return useQuery({ queryKey: ['models', q], queryFn: () => jget<Model[]>(modelsUrl(q)) });
}
export function useModel(id: string | null) {
  return useQuery({ queryKey: ['model', id], queryFn: () => jget<ModelDetail>(`/api/models/${id}`), enabled: !!id });
}
export function useSuggest(query: string, enabled: boolean) {
  return useQuery({
    queryKey: ['suggest', query],
    queryFn: () => jget<Suggestions>(`/api/search/suggest?q=${encodeURIComponent(query)}`),
    enabled,
  });
}
export function useRailTags() {
  return useQuery({ queryKey: ['railTags'], queryFn: () => jget<TagInfo[]>('/api/rail-tags') });
}
export function useTags() { return useQuery({ queryKey: ['tags'], queryFn: () => jget<TagInfo[]>('/api/tags') }); }
export function useGroups() { return useQuery({ queryKey: ['groups'], queryFn: () => jget<GroupInfo[]>('/api/groups') }); }
export function usePins() { return useQuery({ queryKey: ['pins'], queryFn: () => jget<Pin[]>('/api/pins') }); }
export function usePrinterTypes() { return useQuery({ queryKey: ['printerTypes'], queryFn: () => jget<string[]>('/api/printer-types') }); }

/** Invalidate the broad data sets after a mutation. */
export function useInvalidate() {
  const qc = useQueryClient();
  return (keys: string[] = ['models', 'model', 'tags', 'groups', 'pins', 'suggest', 'railTags']) => {
    for (const k of keys) qc.invalidateQueries({ queryKey: [k] });
  };
}

export const api = {
  patchModel: (id: string, patch: Record<string, unknown>) => jsend<ModelDetail>(`/api/models/${id}`, 'PATCH', patch),
  deleteModel: (id: string) => jsend<{ deleted: boolean }>(`/api/models/${id}`, 'DELETE'),
  addTag: (id: string, name: string) => jsend<ModelDetail>(`/api/models/${id}/tags`, 'POST', { name }),
  removeTag: (id: string, name: string) => jsend<ModelDetail>(`/api/models/${id}/tags/${encodeURIComponent(name)}`, 'DELETE'),
  addGroupTo: (id: string, name: string) => jsend<ModelDetail>(`/api/models/${id}/groups`, 'POST', { name }),
  removeGroupFrom: (id: string, name: string) => jsend<ModelDetail>(`/api/models/${id}/groups/${encodeURIComponent(name)}`, 'DELETE'),
  addPrinter: (id: string, name: string) => jsend<ModelDetail>(`/api/models/${id}/printers`, 'POST', { name }),
  removePrinter: (id: string, name: string) => jsend<ModelDetail>(`/api/models/${id}/printers/${encodeURIComponent(name)}`, 'DELETE'),
  createTag: (name: string) => jsend<TagInfo[]>('/api/tags', 'POST', { name }),
  deleteTag: (name: string) => jsend<TagInfo[]>(`/api/tags/${encodeURIComponent(name)}`, 'DELETE'),
  createGroup: (name: string) => jsend<GroupInfo[]>('/api/groups', 'POST', { name }),
  setGroupShared: (name: string, shared: boolean) => jsend<GroupInfo[]>(`/api/groups/${encodeURIComponent(name)}`, 'PATCH', { shared }),
  deleteGroup: (name: string) => jsend<GroupInfo[]>(`/api/groups/${encodeURIComponent(name)}`, 'DELETE'),
  togglePin: (kind: string, name: string) => jsend<Pin[]>('/api/pins', 'POST', { kind, name }),
  addSetting: (id: string, k: string, v: string) => jsend<SettingRow[]>(`/api/models/${id}/settings`, 'POST', { k, v }),
  deleteSetting: (sid: number) => jsend<{ ok: boolean }>(`/api/settings/${sid}`, 'DELETE'),
  saveThumbnail: (id: string, dataUrl: string) => jsend<{ ok: boolean }>(`/api/models/${id}/thumbnail`, 'POST', { dataUrl }),
  scan: (folderPath?: string) => jsend<{ scanned: number; imported: number; skipped: number }>('/api/scan', 'POST', { folderPath }),
  async uploadModel(form: FormData): Promise<Model> {
    const r = await fetch('/api/models', { method: 'POST', body: form });
    if (!r.ok) throw new Error('upload failed');
    return r.json();
  },
  async importProfile(id: string, file: File): Promise<{ imported: string; settings: SettingRow[] }> {
    const fd = new FormData();
    fd.append('file', file);
    const r = await fetch(`/api/models/${id}/settings/import`, { method: 'POST', body: fd });
    if (!r.ok) throw new Error('import failed');
    return r.json();
  },
  async uploadImage(id: string, file: File, kind: string): Promise<ImageItem> {
    const fd = new FormData();
    fd.append('file', file);
    fd.append('kind', kind);
    const r = await fetch(`/api/models/${id}/images`, { method: 'POST', body: fd });
    if (!r.ok) throw new Error('image upload failed');
    return r.json();
  },
};

export { useMutation };
