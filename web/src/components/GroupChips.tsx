import type { ModelDetail } from '../api/client';
import { useGroups, api, useInvalidate } from '../api/client';
import { label, chip } from '../theme';

/** Toggle which groups (folders) this model belongs to. */
export function GroupChips({ model }: { model: ModelDetail }) {
  const { data: groups = [] } = useGroups();
  const invalidate = useInvalidate();
  const toggle = async (name: string, on: boolean) => {
    if (on) await api.removeGroupFrom(model.id, name);
    else await api.addGroupTo(model.id, name);
    invalidate(['model', 'models', 'groups']);
  };
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div style={label}>GROUPS</div>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
        {groups.map((g) => {
          const on = model.groups.includes(g.name);
          return (
            <button key={g.name} onClick={() => toggle(g.name, on)} className="hover-cyan" style={chip(on)}>{g.name}</button>
          );
        })}
      </div>
    </div>
  );
}
