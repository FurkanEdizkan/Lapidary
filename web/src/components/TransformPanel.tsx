import { C, F } from '../theme';

interface Props {
  mode: 'translate' | 'rotate';
  onMode: (m: 'translate' | 'rotate') => void;
  onDrop: () => void;
  onReset: () => void;
  onSave: () => void;
  onCancel: () => void;
  saving: boolean;
}

const btn: React.CSSProperties = {
  background: 'rgba(24,24,28,0.9)', border: `1px solid ${C.border4}`, color: C.textDim,
  fontFamily: F.mono, fontSize: 10, letterSpacing: '0.1em', padding: '6px 10px', borderRadius: 6, cursor: 'pointer',
};

export function TransformPanel(p: Props) {
  const active = (on: boolean): React.CSSProperties => on ? { ...btn, borderColor: C.accent, color: C.accent } : btn;
  return (
    <div style={{ position: 'absolute', left: 14, bottom: 40, display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
      <button style={active(p.mode === 'translate')} onClick={() => p.onMode('translate')}>MOVE</button>
      <button style={active(p.mode === 'rotate')} onClick={() => p.onMode('rotate')}>ROTATE</button>
      <button style={btn} onClick={p.onDrop}>DROP TO PLATE</button>
      <button style={btn} onClick={p.onReset}>RESET</button>
      <button style={{ ...btn, borderColor: C.accent, color: C.accent }} onClick={p.onSave} disabled={p.saving}>{p.saving ? 'SAVING…' : 'SAVE'}</button>
      <button style={btn} onClick={p.onCancel}>CANCEL</button>
    </div>
  );
}
