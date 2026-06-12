import { useRailTags } from '../api/client';
import { useUI } from '../store';
import { chip } from '../theme';

/** Channel-style tag rail: "All" + popular tags for one-click browsing. */
export function TagRail() {
  const { data: tags = [] } = useRailTags();
  const { activeTags, toggleTag, goAll } = useUI();
  const hasFilters = useUI((s) => s.hasFilters());

  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8, padding: '12px 22px 2px', flex: '0 0 auto' }}>
      <button onClick={goAll} className="hover-cyan" style={chip(!hasFilters, 'rail')}>All</button>
      {tags.map((t) => (
        <button key={t.name} onClick={() => toggleTag(t.name)} className="hover-cyan" style={chip(activeTags.includes(t.name), 'rail')}>{t.name}</button>
      ))}
    </div>
  );
}
