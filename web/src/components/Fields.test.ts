import { expect, test } from 'vitest'
import { proposeKey } from './Fields'

test('a key is proposed from a label, with Turkish and accented letters folded to plain ones', () => {
  expect(proposeKey('Stock count')).toBe('stock_count')
  expect(proposeKey('Tedarikçi / Şube')).toBe('tedarikci_sube')
  expect(proposeKey('Işık ölçüsü (mm)')).toBe('isik_olcusu_mm')
  expect(proposeKey('   ')).toBe('')
  expect(proposeKey('x'.repeat(60))).toHaveLength(40)
})
