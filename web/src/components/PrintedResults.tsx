import { useRef } from 'react';
import type { ModelDetail } from '../api/client';
import { api, useInvalidate } from '../api/client';
import { C, F, label } from '../theme';

/** Two drop slots for printed/painted result photos. */
export function PrintedResults({ model }: { model: ModelDetail }) {
  const printed = model.images.filter((i) => i.kind === 'printed');
  const painted = model.images.filter((i) => i.kind !== 'printed');
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div style={label}>PRINTED RESULTS</div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
        <Slot model={model} kind="printed" image={printed[0]} placeholder="Drop photo — printed" />
        <Slot model={model} kind="painted" image={painted[0]} placeholder="Drop photo — painted / finished" />
      </div>
    </div>
  );
}

function Slot({ model, kind, image, placeholder }: { model: ModelDetail; kind: string; image?: { id: number; url: string }; placeholder: string }) {
  const ref = useRef<HTMLInputElement | null>(null);
  const invalidate = useInvalidate();
  const upload = async (file: File) => { await api.uploadImage(model.id, file, kind); invalidate(['model']); };
  return (
    <div
      onClick={() => ref.current?.click()}
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => { e.preventDefault(); const f = e.dataTransfer.files?.[0]; if (f) upload(f); }}
      className="hover-cyan"
      style={{ width: '100%', height: 150, borderRadius: 10, border: `1.5px dashed ${C.border4}`, background: image ? '#000' : '#1c1c20', cursor: 'pointer', overflow: 'hidden', display: 'grid', placeItems: 'center', color: C.textMute2, fontSize: 11.5, textAlign: 'center', padding: 8 }}
    >
      {image ? (
        <img src={image.url} alt={kind} style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
      ) : (
        <span style={{ fontFamily: F.mono, fontSize: 10.5 }}>{placeholder}</span>
      )}
      <input ref={ref} type="file" accept="image/*" style={{ display: 'none' }} onChange={(e) => { const f = e.target.files?.[0]; if (f) upload(f); e.target.value = ''; }} />
    </div>
  );
}
