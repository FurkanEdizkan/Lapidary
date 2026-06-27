import { create } from 'zustand';

export type ViewMode = 'grid' | 'cards' | 'list';

interface UIState {
  query: string;
  activeTags: string[];
  groupScope: string | null;
  creatorScope: string | null;
  searchFocus: boolean;
  view: ViewMode;
  selectedId: string | null;
  inspectId: string | null;
  showAdd: boolean;
  showManager: boolean;

  setQuery: (q: string) => void;
  setSearchFocus: (f: boolean) => void;
  toggleTag: (t: string) => void;
  addTagFilter: (t: string) => void;
  setGroupScope: (g: string | null) => void;
  setCreatorScope: (c: string | null) => void;
  setView: (v: ViewMode) => void;
  open: (id: string | null) => void;
  openInspect: (id: string) => void;
  closeInspect: () => void;
  setShowAdd: (b: boolean) => void;
  setShowManager: (b: boolean) => void;
  clearFilters: () => void;
  goAll: () => void;
  hasFilters: () => boolean;
}

export const useUI = create<UIState>((set, get) => ({
  query: '',
  activeTags: [],
  groupScope: null,
  creatorScope: null,
  searchFocus: false,
  view: 'grid',
  selectedId: null,
  inspectId: null,
  showAdd: false,
  showManager: false,

  setQuery: (q) => set({ query: q, searchFocus: true }),
  setSearchFocus: (f) => set({ searchFocus: f }),
  toggleTag: (t) =>
    set((s) => ({ activeTags: s.activeTags.includes(t) ? s.activeTags.filter((x) => x !== t) : [...s.activeTags, t], searchFocus: false })),
  addTagFilter: (t) =>
    set((s) => ({ activeTags: s.activeTags.includes(t) ? s.activeTags : [...s.activeTags, t], query: '', searchFocus: false })),
  setGroupScope: (g) => set((s) => ({ groupScope: s.groupScope === g ? null : g })),
  setCreatorScope: (c) => set((s) => ({ creatorScope: s.creatorScope === c ? null : c, query: '', searchFocus: false })),
  setView: (v) => set({ view: v }),
  // Closing the detail (id===null) also closes any open Inspect; switching detail keeps it.
  open: (id) => set((s) => ({ selectedId: id, inspectId: id ? s.inspectId : null, searchFocus: false })),
  openInspect: (id) => set({ inspectId: id }),
  closeInspect: () => set({ inspectId: null }),
  setShowAdd: (b) => set({ showAdd: b }),
  setShowManager: (b) => set({ showManager: b }),
  clearFilters: () => set({ query: '', activeTags: [], groupScope: null, creatorScope: null }),
  goAll: () => set({ groupScope: null, creatorScope: null, activeTags: [], query: '' }),
  hasFilters: () => {
    const s = get();
    return !!(s.query || s.activeTags.length || s.groupScope || s.creatorScope);
  },
}));
