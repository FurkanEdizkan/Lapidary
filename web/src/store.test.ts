import { describe, it, expect, beforeEach } from 'vitest';
import { useUI } from './store';

describe('useUI inspect state', () => {
  beforeEach(() => { useUI.setState({ selectedId: null, inspectId: null }); });

  it('openInspect sets inspectId', () => {
    useUI.getState().openInspect('m1');
    expect(useUI.getState().inspectId).toBe('m1');
  });

  it('closeInspect clears inspectId', () => {
    useUI.getState().openInspect('m1');
    useUI.getState().closeInspect();
    expect(useUI.getState().inspectId).toBeNull();
  });

  it('open(null) (closing the detail) also clears an open inspect', () => {
    useUI.setState({ selectedId: 'm1' });
    useUI.getState().openInspect('m1');
    useUI.getState().open(null);
    expect(useUI.getState().selectedId).toBeNull();
    expect(useUI.getState().inspectId).toBeNull();
  });

  it('open(id) (switching detail) preserves an open inspect', () => {
    useUI.getState().openInspect('m1');
    useUI.getState().open('m1');
    expect(useUI.getState().inspectId).toBe('m1');
  });
});
