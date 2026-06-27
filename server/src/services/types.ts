/** Shared DTOs returned by the API. The frontend formats labels/styles from these. */
export interface ModelDTO {
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

export interface SettingRow {
  id: number;
  k: string;
  v: string;
  source: string;
}

export interface ImageDTO {
  id: number;
  url: string;
  caption: string | null;
  kind: string;
}

export interface PlateTransform {
  position: [number, number, number];
  quaternion: [number, number, number, number];
  scale: number;
}

export interface ModelDetailDTO extends ModelDTO {
  notes: string | null;
  settings: SettingRow[];
  images: ImageDTO[];
  transform: PlateTransform | null;
}

export interface ModelFilter {
  search?: string;
  tags?: string[];
  group?: string;
  creator?: string;
  sort?: string;
}
