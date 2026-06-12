import { useSuggest, usePins, api, useInvalidate } from '../api/client';
import { useUI } from '../store';
import { C, F, chip } from '../theme';

/** The search dropdown: MODELS / CREATORS / TAGS sections, mirroring the prototype. */
export function SearchSuggest() {
  const { query, addTagFilter, setCreatorScope, open } = useUI();
  const { data } = useSuggest(query.trim(), true);
  const { data: pins = [] } = usePins();
  const invalidate = useInvalidate();
  const isPinned = (kind: string, name: string) => pins.some((p) => p.kind === kind && p.name === name);
  const pin = async (kind: string, name: string) => { await api.togglePin(kind, name); invalidate(['pins']); };

  if (!data) return null;
  const noHits = !!query.trim() && !data.models.length && !data.creators.length && !data.tags.length;

  return (
    <div
      onMouseDown={(e) => e.preventDefault()}
      style={{ position: 'absolute', top: 'calc(100% + 8px)', left: 0, right: 0, background: C.surface, border: `1px solid ${C.border2}`, borderRadius: 12, boxShadow: '0 28px 64px rgba(0,0,0,0.65)', zIndex: 80, maxHeight: 460, overflowY: 'auto', padding: 8, display: 'flex', flexDirection: 'column', gap: 10 }}
    >
      {data.models.length > 0 && (
        <Section title="MODELS">
          {data.models.map((m) => (
            <button key={m.id} onClick={() => open(m.id)} className="hover-row" style={rowBtn}>
              <div style={{ width: 42, height: 36, borderRadius: 6, background: C.surfaceInput, border: `1px solid ${C.border2}`, flex: '0 0 auto', overflow: 'hidden' }}>
                <div style={{ width: '100%', height: '100%', backgroundImage: `url(/api/models/${m.id}/thumbnail)`, backgroundSize: 'contain', backgroundPosition: 'center', backgroundRepeat: 'no-repeat' }} />
              </div>
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ fontSize: 12.5, fontWeight: 600, color: C.text, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.name}</div>
                <div style={{ fontSize: 10.5, color: C.textMute }}>{m.creator} · {m.type}</div>
              </div>
            </button>
          ))}
        </Section>
      )}

      {data.creators.length > 0 && (
        <Section title="CREATORS">
          {data.creators.map((c) => (
            <div key={c.name} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <button onClick={() => setCreatorScope(c.name)} style={{ ...rowBtn, flex: 1 }}>
                <span style={{ color: C.accent, fontSize: 10 }}>◆</span>
                <span style={{ fontSize: 12.5, fontWeight: 600, color: C.text, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.name}</span>
                <span style={{ fontFamily: F.mono, fontSize: 10, color: C.textMute2 }}>{c.count} models</span>
              </button>
              <button onClick={() => pin('creator', c.name)} title="Pin creator" className="hover-cyan" style={{ background: 'none', border: 'none', color: C.textMute2, cursor: 'pointer', fontSize: 13, padding: '4px 8px', flex: '0 0 auto' }}>{isPinned('creator', c.name) ? '★' : '☆'}</button>
            </div>
          ))}
        </Section>
      )}

      {data.tags.length > 0 && (
        <Section title={data.tagHeader}>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, padding: '0 8px 6px' }}>
            {data.tags.map((t) => (
              <button key={t.name} onClick={() => addTagFilter(t.name)} className="hover-cyan" style={chip(false)}>
                {t.name}<span style={{ opacity: 0.55 }}>{t.count}</span>
              </button>
            ))}
          </div>
        </Section>
      )}

      {noHits && <div style={{ padding: '14px 10px', fontSize: 12, color: C.textMute2, textAlign: 'center' }}>No matches in the library.</div>}
    </div>
  );
}

const rowBtn: React.CSSProperties = {
  display: 'flex', alignItems: 'center', gap: 10, width: '100%', background: 'transparent', border: 'none',
  borderRadius: 8, padding: '6px 8px', cursor: 'pointer', textAlign: 'left',
};

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <div style={{ fontFamily: F.mono, fontSize: 9, letterSpacing: '0.2em', color: C.textFaint, padding: '4px 8px' }}>{title}</div>
      {children}
    </div>
  );
}
