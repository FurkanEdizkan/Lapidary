import type { Model } from '../api/client';
import { ModelTile } from './ModelTile';

export function GridView({ models }: { models: Model[] }) {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(234px, 1fr))', gap: 10 }}>
      {models.map((m) => (
        <ModelTile key={m.id} model={m} />
      ))}
    </div>
  );
}
