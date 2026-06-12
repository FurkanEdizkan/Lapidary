import { useState } from 'react';
import { usePins, useGroups, useTags, useModels, api, useInvalidate } from '../api/client';
import { useUI } from '../store';
import { C, F, chip } from '../theme';

/** Pinned favorites (creators/tags/groups) + Groups chips with share badge + new-group input. */
export function PinnedGroupsBar() {
  const { activeTags, groupScope, creatorScope, toggleTag, setCreatorScope, setGroupScope } = useUI();
  const { data: pins = [] } = usePins();
  const { data: groups = [] } = useGroups();
  const { data: tags = [] } = useTags();
  const { data: allModels = [] } = useModels({});
  const invalidate = useInvalidate();
  const [newGroup, setNewGroup] = useState('');

  const creatorCounts = new Map<string, number>();
  for (const m of allModels) creatorCounts.set(m.creator, (creatorCounts.get(m.creator) || 0) + 1);
  const tagCount = (n: string) => tags.find((t) => t.name === n)?.count ?? 0;
  const groupCount = (n: string) => groups.find((g) => g.name === n)?.count ?? 0;

  const countFor = (kind: string, name: string) =>
    kind === 'creator' ? creatorCounts.get(name) || 0 : kind === 'tag' ? tagCount(name) : groupCount(name);
  const isActive = (kind: string, name: string) =>
    (kind === 'creator' && creatorScope === name) || (kind === 'tag' && activeTags.includes(name)) || (kind === 'group' && groupScope === name);
  const selectPin = (kind: string, name: string) => {
    if (kind === 'creator') setCreatorScope(name);
    else if (kind === 'tag') toggleTag(name);
    else setGroupScope(name);
  };
  const unpin = async (kind: string, name: string) => { await api.togglePin(kind, name); invalidate(['pins']); };
  const pinGroup = async (name: string) => { await api.togglePin('group', name); invalidate(['pins']); };
  const createGroup = async () => {
    const n = newGroup.trim();
    if (!n) return;
    await api.createGroup(n);
    setNewGroup('');
    invalidate(['groups']);
  };
  const isGroupPinned = (name: string) => pins.some((p) => p.kind === 'group' && p.name === name);

  return (
    <div style={{ display: 'flex', alignItems: 'center', flexWrap: 'wrap', gap: 8, padding: '12px 22px 0', flex: '0 0 auto' }}>
      <span style={{ ...railLabel, marginRight: 2 }}>PINNED</span>
      {pins.map((p) => (
        <span key={`${p.kind}-${p.name}`} style={chip(isActive(p.kind, p.name), 'bar')}>
          <button onClick={() => selectPin(p.kind, p.name)} style={innerBtn}>
            <span style={{ fontSize: 10, opacity: 0.7 }}>{p.kind === 'creator' ? '◆' : p.kind === 'tag' ? '#' : '▣'}</span>
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{p.name}</span>
            <span style={{ fontFamily: F.mono, fontSize: 10, opacity: 0.6 }}>{countFor(p.kind, p.name)}</span>
          </button>
          <button onClick={() => unpin(p.kind, p.name)} title="Unpin" style={{ ...xBtn }}>✕</button>
        </span>
      ))}
      {pins.length === 0 && <span style={{ fontSize: 11.5, color: C.textFaint }}>pin creators, tags or groups from search ☆</span>}

      <div style={{ width: 1, height: 18, background: C.border2, margin: '0 8px' }} />

      <span style={{ ...railLabel, marginRight: 2 }}>GROUPS</span>
      {groups.map((g) => (
        <span key={g.name} style={chip(groupScope === g.name, 'bar')}>
          <button onClick={() => setGroupScope(g.name)} style={innerBtn}>
            <span style={{ fontSize: 10, opacity: 0.7 }}>▣</span>
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{g.name}</span>
            {g.shared && <span style={{ fontFamily: F.mono, fontSize: 7.5, letterSpacing: '0.12em', opacity: 0.65, border: '1px solid currentColor', borderRadius: 4, padding: '1px 4px' }}>SHARED</span>}
            <span style={{ fontFamily: F.mono, fontSize: 10, opacity: 0.6 }}>{g.count}</span>
          </button>
          <button onClick={() => pinGroup(g.name)} title="Pin group" style={{ ...xBtn, fontSize: 11 }}>{isGroupPinned(g.name) ? '★' : '☆'}</button>
        </span>
      ))}
      <input
        value={newGroup}
        onChange={(e) => setNewGroup(e.target.value)}
        onKeyDown={(e) => { if (e.key === 'Enter') createGroup(); }}
        placeholder="+ new group ↵"
        className="hover-cyan"
        style={{ background: 'transparent', border: `1px dashed ${C.border4}`, borderRadius: 99, color: C.text, fontSize: 11.5, padding: '5px 12px', outline: 'none', width: 118 }}
      />
    </div>
  );
}

const railLabel: React.CSSProperties = { fontFamily: F.mono, fontSize: 9, letterSpacing: '0.2em', color: C.textFaint };
const innerBtn: React.CSSProperties = { display: 'inline-flex', alignItems: 'center', gap: 7, background: 'none', border: 'none', color: 'inherit', font: 'inherit', cursor: 'pointer', padding: 0, minWidth: 0 };
const xBtn: React.CSSProperties = { background: 'none', border: 'none', color: 'inherit', opacity: 0.5, cursor: 'pointer', fontSize: 10, padding: '0 0 0 2px', lineHeight: 1 };
