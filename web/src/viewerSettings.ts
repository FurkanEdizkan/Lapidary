import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/** Selectable measurement-grid cell sizes (mm). */
export const GRID_CELLS = [5, 10, 20, 50] as const;
export type GridCell = (typeof GRID_CELLS)[number];

interface ViewerSettings {
  /** Measurement-grid cell size in millimetres (a cell square = this many mm). */
  gridCellMm: GridCell;
  /** Whether the measurement grid is shown in the 3D viewer. */
  showGrid: boolean;
  setGridCellMm: (mm: GridCell) => void;
  setShowGrid: (on: boolean) => void;
  toggleGrid: () => void;
}

/**
 * Persisted viewer preferences (localStorage). These are the DEFAULTS applied to every
 * Inspect viewer; changing them from the viewport updates the default for next time.
 */
export const useViewerSettings = create<ViewerSettings>()(
  persist(
    (set) => ({
      gridCellMm: 10,
      showGrid: true,
      setGridCellMm: (mm) => set({ gridCellMm: mm }),
      setShowGrid: (on) => set({ showGrid: on }),
      toggleGrid: () => set((s) => ({ showGrid: !s.showGrid })),
    }),
    { name: 'lapidary-viewer-settings' },
  ),
);
