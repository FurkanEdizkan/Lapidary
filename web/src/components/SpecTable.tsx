import type { ModelDetail } from '../api/client';
import { sizeLabel, fmtDate, fileMB } from '../lib/format';
import { C, F, label } from '../theme';

/** Read-only specs grid for the detail view. */
export function SpecTable({ model }: { model: ModelDetail }) {
  const rows: [string, string][] = [
    ['Type', model.type],
    ['Dimensions', sizeLabel(model)],
    ['File', `${model.format} · ${fileMB(model.fileSizeBytes)}`],
    ['Designed', fmtDate(model.created)],
    ['Added to library', fmtDate(model.added)],
    ['Triangles', model.triangleCount ? model.triangleCount.toLocaleString() : '—'],
  ];
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <div style={{ ...label, marginBottom: 6 }}>SPECS</div>
      {rows.map(([k, v]) => (
        <div key={k} style={{ display: 'grid', gridTemplateColumns: '140px 1fr', gap: 10, padding: '6px 0', borderBottom: '1px solid #232327', fontSize: 12.5 }}>
          <span style={{ color: '#8b8b93' }}>{k}</span>
          <span style={{ color: '#e0e0e5', fontFamily: F.mono, fontSize: 11.5 }}>{v}</span>
        </div>
      ))}
    </div>
  );
}
