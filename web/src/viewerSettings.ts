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
  /** Whether the live bounding-box (with W×D×H labels) is shown. */
  showBoundingBox: boolean;
  /** Selected build-plate preset id ('follow-printer' | 'auto' | 'custom' | a preset id). */
  platePresetId: string;
  /** Dimensions used when platePresetId === 'custom'. */
  customPlate: { widthMm: number; depthMm: number };

  setGridCellMm: (mm: GridCell) => void;
  setShowGrid: (on: boolean) => void;
  toggleGrid: () => void;
  setShowBoundingBox: (on: boolean) => void;
  toggleBoundingBox: () => void;
  setPlatePresetId: (id: string) => void;
  setCustomPlate: (p: { widthMm: number; depthMm: number }) => void;
}

/**
 * Persisted viewer preferences (localStorage). These are the DEFAULTS applied to every
 * Inspect viewer; changing them from the viewport updates the default for next time.
 * 'follow-printer' re-derives the plate per model from its compatible printers.
 */
export const useViewerSettings = create<ViewerSettings>()(
  persist(
    (set) => ({
      gridCellMm: 10,
      showGrid: true,
      showBoundingBox: true,
      platePresetId: 'follow-printer',
      customPlate: { widthMm: 220, depthMm: 220 },

      setGridCellMm: (mm) => set({ gridCellMm: mm }),
      setShowGrid: (on) => set({ showGrid: on }),
      toggleGrid: () => set((s) => ({ showGrid: !s.showGrid })),
      setShowBoundingBox: (on) => set({ showBoundingBox: on }),
      toggleBoundingBox: () => set((s) => ({ showBoundingBox: !s.showBoundingBox })),
      setPlatePresetId: (id) => set({ platePresetId: id }),
      setCustomPlate: (p) => set({ customPlate: p }),
    }),
    { name: 'lapidary-viewer-settings' },
  ),
);
