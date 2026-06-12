import { useState } from 'react';
import { useTags, useGroups, usePins, api, useInvalidate } from '../api/client';
import { useUI } from '../store';
import { C, F, label } from '../theme';
import { modalWrap, modalBox, Header } from './AddModelModal';

export function TagsGroupsManager() {
  const show = useUI((s) => s.showManager);
  const close = () => useUI.getState().setShowManager(false);
  const { addTagFilter, setGroupScope } = useUI();
  const { data: tags = [] } = useTags();
  const { data: groups = [] } = useGroups();
  const { data: pins = [] } = usePins();
  const invalidate = useInvalidate();
  const [newTag, setNewTag] = useState('');
  const [newGroup, setNewGroup] = useState('');

  if (!show) return null;
  const isPinned = (kind: string, name: string) => pins.some((p) => p.kind === kind && p.name === name);
  const pin = async (kind: string, name: string) => { await api.togglePin(kind, name); invalidate(['pins']); };

  const createTag = async () => { if (!newTag.trim()) return; await api.createTag(newTag); setNewTag(''); invalidate(['tags', 'railTags']); };
  const deleteTag = async (n: string) => { await api.deleteTag(n); invalidate(['tags', 'railTags', 'models', 'pins']); };
  const createGroup = async () => { if (!newGroup.trim()) return; await api.createGroup(newGroup); setNewGroup(''); invalidate(['groups']); };
  const shareGroup = async (n: string, shared: boolean) => { await api.setGroupShared(n, !shared); invalidate(['groups']); };
  const deleteGroup = async (n: string) => { await api.deleteGroup(n); invalidate(['groups', 'models', 'pins']); };

  return (
    <div onClick={close} style={modalWrap}>
      <div onClick={(e) => e.stopPropagation()} style={{ ...modalBox, width: 'min(560px, 94vw)', maxHeight: '88vh', gap: 20 }}>
        <Header title="Tags ⁄ Groups" onClose={close} />

        <div style={{ display: 'flex', flexDirection: 'column', gap: 9 }}>
          <div style={label}>TAGS</div>
          <div style={{ display: 'flex', gap: 8 }}>
            <input value={newTag} onChange={(e) => setNewTag(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') createTag(); }} placeholder="New tag, e.g. dragon" style={{ ...inp, flex: 1, fontFamily: F.mono }} />
            <button onClick={createTag} style={createBtn}>Create</button>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            {tags.map((t) => (
              <Row key={t.name}>
                <span style={{ fontFamily: F.mono, fontSize: 12, color: '#dcdce1', flex: 1 }}>{t.name}</span>
                <span style={{ fontFamily: F.mono, fontSize: 10.5, color: C.textFaint2 }}>{t.count} models</span>
                <IconBtn onClick={() => pin('tag', t.name)} title="Pin tag">{isPinned('tag', t.name) ? '★' : '☆'}</IconBtn>
                <SmallBtn onClick={() => { addTagFilter(t.name); close(); }}>filter</SmallBtn>
                <IconBtn onClick={() => deleteTag(t.name)} danger>✕</IconBtn>
              </Row>
            ))}
          </div>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: 9 }}>
          <div style={label}>GROUPS <span style={{ letterSpacing: '0.05em', color: '#5a5a61' }}>— personal & shared folders</span></div>
          <div style={{ display: 'flex', gap: 8 }}>
            <input value={newGroup} onChange={(e) => setNewGroup(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') createGroup(); }} placeholder="New group, e.g. Dragon Hoard" style={{ ...inp, flex: 1 }} />
            <button onClick={createGroup} style={createBtn}>Create</button>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            {groups.map((g) => (
              <Row key={g.name}>
                <span style={{ fontSize: 12.5, color: '#dcdce1', flex: 1, display: 'inline-flex', alignItems: 'center', gap: 8 }}>
                  {g.name}
                  {g.shared && <span style={{ fontFamily: F.mono, fontSize: 7.5, letterSpacing: '0.14em', color: C.textMute2, border: '1px solid #303036', borderRadius: 4, padding: '1px 4px' }}>SHARED</span>}
                </span>
                <span style={{ fontFamily: F.mono, fontSize: 10.5, color: C.textFaint2 }}>{g.count} models</span>
                <SmallBtn onClick={() => shareGroup(g.name, g.shared)}>{g.shared ? 'make personal' : 'share'}</SmallBtn>
                <SmallBtn onClick={() => { setGroupScope(g.name); close(); }}>open</SmallBtn>
                <IconBtn onClick={() => deleteGroup(g.name)} danger>✕</IconBtn>
              </Row>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

const inp: React.CSSProperties = { background: C.surface2, border: `1px solid ${C.border2}`, borderRadius: 8, color: C.text, fontSize: 12.5, padding: '8px 11px', outline: 'none' };
const createBtn: React.CSSProperties = { background: C.accent, border: 'none', color: C.onAccent, borderRadius: 8, padding: '8px 15px', fontSize: 12, fontWeight: 700, cursor: 'pointer' };

function Row({ children }: { children: React.ReactNode }) {
  return <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '7px 4px', borderBottom: '1px solid #222226' }}>{children}</div>;
}
function SmallBtn({ children, onClick }: { children: React.ReactNode; onClick: () => void }) {
  return <button onClick={onClick} className="hover-cyan" style={{ background: 'transparent', border: `1px solid ${C.border4}`, color: '#b3b3ba', borderRadius: 6, padding: '3px 9px', fontSize: 10.5, cursor: 'pointer' }}>{children}</button>;
}
function IconBtn({ children, onClick, title, danger }: { children: React.ReactNode; onClick: () => void; title?: string; danger?: boolean }) {
  return <button onClick={onClick} title={title} className={danger ? '' : 'hover-cyan'} style={{ background: 'transparent', border: 'none', color: danger ? C.textFaint2 : C.textMute2, cursor: 'pointer', fontSize: 12, padding: '2px 4px' }}>{children}</button>;
}
