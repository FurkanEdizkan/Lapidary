import { render, screen } from '@testing-library/react'
import { expect, test } from 'vitest'
import { MeasureBar } from './Measure'
import { strings } from '../lib/strings'

test('a measured value carries ≈ exactly when it is approximate', () => {
  const noop = () => undefined
  const { rerender } = render(
    <MeasureBar tool="diameter" onTool={noop} reading={{ value: 22, approximate: false }} note={null} />,
  )
  expect(screen.getByText('22.000 mm').getAttribute('title')).toBe(strings.detail.exactTitle)
  expect(screen.queryByText(strings.detail.approximate)).toBeNull()

  rerender(<MeasureBar tool="wall" onTool={noop} reading={{ value: 19.987, approximate: true }} note={null} />)
  expect(screen.getByText('19.987 mm').getAttribute('title')).toBe(strings.detail.approximateTitle)
  expect(screen.getByText(strings.detail.approximate)).toBeTruthy()

  rerender(<MeasureBar tool="angle" onTool={noop} reading={{ value: 90, approximate: false }} note={null} />)
  expect(screen.getByText('90.000°')).toBeTruthy()
  expect(screen.queryByText(strings.detail.approximate)).toBeNull()
})
