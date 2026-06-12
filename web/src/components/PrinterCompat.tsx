import type { ModelDetail } from '../api/client';
import { usePrinterTypes, api, useInvalidate } from '../api/client';
import { C, F, label } from '../theme';

/** Printer compatibility chips. Click toggles a printer on/off for this model. */
export function PrinterCompat({ model }: { model: ModelDetail }) {
  const { data: printers = [] } = usePrinterTypes();
  const invalidate = useInvalidate();
  const toggle = async (name: string, on: boolean) => {
    if (on) await api.removePrinter(model.id, name);
    else await api.addPrinter(model.id, name);
    invalidate(['model', 'models']);
  };
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div style={label}>PRINTER COMPATIBILITY</div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
        {printers.map((p) => {
          const on = model.printers.includes(p);
          return (
            <button
              key={p}
              onClick={() => toggle(p, on)}
              style={{
                padding: '5px 12px', borderRadius: 7, fontSize: 11, fontFamily: F.mono, cursor: 'pointer',
                border: `1px solid ${on ? '#3a3a42' : C.border}`, background: on ? '#242429' : 'transparent',
                color: on ? '#e0e0e5' : '#5a5a61', textDecoration: on ? 'none' : 'line-through',
              }}
            >
              {p}
            </button>
          );
        })}
      </div>
    </div>
  );
}
