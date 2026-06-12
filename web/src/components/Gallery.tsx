import { useModels } from '../api/client';
import { useUI } from '../store';
import { C, F } from '../theme';
import { GridView } from './GridView';
import { CardsView } from './CardsView';
import { ListView } from './ListView';

export function Gallery() {
  const { query, activeTags, groupScope, creatorScope, view, toggleTag, setCreatorScope, clearFilters } = useUI();
  const hasFilters = useUI((s) => s.hasFilters());
  const { data: models = [], isLoading } = useModels({ search: query, tags: activeTags, group: groupScope, creator: creatorScope });

  const scopeTitle = groupScope || creatorScope || 'All models';
  const countLabel = `${models.length} ${models.length === 1 ? 'MODEL' : 'MODELS'}${query ? ` · “${query.trim()}”` : ''}`;

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '16px 22px 40px', minHeight: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 10, marginBottom: 14 }}>
        <div style={{ fontSize: 19, fontWeight: 700, letterSpacing: '-0.01em' }}>{scopeTitle}</div>
        <div style={{ fontFamily: F.mono, fontSize: 11, color: C.textMute2 }}>{countLabel}</div>
        {activeTags.map((t) => (
          <FilterChip key={`t-${t}`} label={`#${t}`} onRemove={() => toggleTag(t)} />
        ))}
        {creatorScope && <FilterChip label={`◆ ${creatorScope}`} onRemove={() => setCreatorScope(creatorScope)} />}
        {hasFilters && (
          <button onClick={clearFilters} className="hover-cyan" style={{ background: 'transparent', border: 'none', color: C.textMute, fontSize: 11.5, cursor: 'pointer', textDecoration: 'underline', textUnderlineOffset: 3, padding: 0 }}>clear all</button>
        )}
      </div>

      {!isLoading && models.length === 0 ? (
        <div style={{ padding: '70px 20px', textAlign: 'center', color: C.textMute2 }}>
          <div style={{ fontSize: 30, opacity: 0.5, marginBottom: 10 }}>◇</div>
          <div style={{ fontSize: 14, fontWeight: 600, color: '#b3b3ba' }}>No models match</div>
          <div style={{ fontSize: 12, marginTop: 4 }}>
            Try a different search, or{' '}
            <button onClick={clearFilters} style={{ background: 'none', border: 'none', color: C.accent, cursor: 'pointer', textDecoration: 'underline', textUnderlineOffset: 3, fontSize: 12, padding: 0 }}>clear all filters</button>
          </div>
        </div>
      ) : view === 'grid' ? (
        <GridView models={models} />
      ) : view === 'cards' ? (
        <CardsView models={models} />
      ) : (
        <ListView models={models} />
      )}
    </div>
  );
}

function FilterChip({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 7, padding: '4px 11px', borderRadius: 99, fontSize: 11, fontFamily: F.mono, color: C.onAccent, background: C.accent, border: `1px solid ${C.accent}` }}>
      {label}
      <button onClick={onRemove} style={{ background: 'none', border: 'none', color: C.onAccent, opacity: 0.6, cursor: 'pointer', padding: 0, fontSize: 11, lineHeight: 1 }}>✕</button>
    </span>
  );
}
