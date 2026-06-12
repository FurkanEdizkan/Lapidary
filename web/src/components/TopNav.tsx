import { useRef } from 'react';
import { useUI, type ViewMode } from '../store';
import { useSuggest } from '../api/client';
import { C, F } from '../theme';
import { SearchSuggest } from './SearchSuggest';

export function TopNav() {
  const { query, setQuery, searchFocus, setSearchFocus, view, setView, setShowAdd, setShowManager } = useUI();
  const blurTimer = useRef<number | undefined>(undefined);

  // Drives whether the dropdown shows (empty query still shows popular tags).
  const { data: sugg } = useSuggest(query.trim(), searchFocus);
  const showPanel = searchFocus && (!!query.trim() || (sugg ? sugg.tags.length > 0 : true));

  return (
    <header style={{ display: 'flex', alignItems: 'center', gap: 18, padding: '0 22px', height: 58, background: C.nav, borderBottom: `1px solid ${C.border}`, flex: '0 0 auto' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flex: '0 0 auto' }}>
        <div style={{ width: 24, height: 24, border: `1.5px solid ${C.accent}`, transform: 'rotate(45deg)', display: 'grid', placeItems: 'center' }}>
          <div style={{ width: 8, height: 8, background: C.accent }} />
        </div>
        <div style={{ fontWeight: 800, letterSpacing: '0.14em', fontSize: 14 }}>LAPIDARY</div>
      </div>

      <div style={{ position: 'relative', flex: 1, maxWidth: 620, margin: '0 auto' }}>
        <div style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)', color: C.textFaint, fontSize: 13, pointerEvents: 'none' }}>⌕</div>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onFocus={() => { window.clearTimeout(blurTimer.current); setSearchFocus(true); }}
          onBlur={() => { blurTimer.current = window.setTimeout(() => setSearchFocus(false), 140); }}
          placeholder="Search models, tags, creators — try “dragon”"
          style={{ width: '100%', background: C.surface2, border: `1px solid ${C.border2}`, borderRadius: 99, color: C.text, fontSize: 13, padding: '9px 14px 9px 33px', outline: 'none' }}
        />
        {showPanel && <SearchSuggest />}
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flex: '0 0 auto' }}>
        <div style={{ display: 'flex', gap: 2, background: C.surface2, border: `1px solid ${C.border2}`, borderRadius: 9, padding: 3 }}>
          {(['grid', 'cards', 'list'] as ViewMode[]).map((v) => (
            <button key={v} onClick={() => setView(v)} style={segBtn(view === v)}>{v[0].toUpperCase() + v.slice(1)}</button>
          ))}
        </div>
        <button onClick={() => setShowManager(true)} className="hover-cyan" style={{ background: 'transparent', border: `1px solid ${C.border4}`, color: C.textDim, borderRadius: 9, padding: '8px 14px', fontSize: 12.5, fontWeight: 600, cursor: 'pointer' }}>Tags ⁄ Groups</button>
        <button onClick={() => setShowAdd(true)} style={{ background: C.accent, border: `1px solid ${C.accent}`, color: C.onAccent, borderRadius: 9, padding: '8px 16px', fontSize: 12.5, fontWeight: 700, cursor: 'pointer' }}>+ Add model</button>
      </div>
    </header>
  );
}

function segBtn(active: boolean): React.CSSProperties {
  return {
    background: active ? '#2e2e34' : 'transparent', border: 'none', color: active ? '#f2f2f4' : C.textMute,
    borderRadius: 7, padding: '6px 13px', fontSize: 12, fontWeight: 600, cursor: 'pointer',
  };
}
