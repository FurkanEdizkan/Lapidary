import { fireEvent, render, screen, within } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { ExplodeBar, MeasureBar, SectionBar } from './Measure'
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

/** The section controls hand back the whole cut each time: its axis, where it is, and which side stays. */
test('the section bar turns a cut on along an axis, moves it, flips it and turns it off', () => {
  const onSection = vi.fn()
  const { rerender } = render(<SectionBar section={null} onSection={onSection} closed />)
  const toolbar = screen.getByRole('toolbar', { name: strings.section.label })
  expect(within(toolbar).getByRole('button', { name: strings.section.off }).getAttribute('aria-pressed')).toBe('true')
  // Nothing to move or flip until there is a cut.
  expect(screen.queryByLabelText(strings.section.position)).toBeNull()
  fireEvent.click(within(toolbar).getByRole('button', { name: strings.section.axes.z }))
  expect(onSection).toHaveBeenLastCalledWith({ axis: 'z', at: 0.5, flip: false })

  rerender(<SectionBar section={{ axis: 'z', at: 0.5, flip: false }} onSection={onSection} closed />)
  expect(within(toolbar).getByRole('button', { name: strings.section.axes.z }).getAttribute('aria-pressed')).toBe('true')
  fireEvent.change(screen.getByLabelText(strings.section.position), { target: { value: '250' } })
  expect(onSection).toHaveBeenLastCalledWith({ axis: 'z', at: 0.25, flip: false })
  fireEvent.click(within(toolbar).getByRole('button', { name: strings.section.flip }))
  expect(onSection).toHaveBeenLastCalledWith({ axis: 'z', at: 0.5, flip: true })
  // Another axis keeps where the cut was and which side stays.
  fireEvent.click(within(toolbar).getByRole('button', { name: strings.section.axes.x }))
  expect(onSection).toHaveBeenLastCalledWith({ axis: 'x', at: 0.5, flip: false })
  fireEvent.click(within(toolbar).getByRole('button', { name: strings.section.off }))
  expect(onSection).toHaveBeenLastCalledWith(null)
})

test('a cut says it shows no filled face on a mesh measured open, or never measured, and nothing on a closed one', () => {
  const cut = { axis: 'z' as const, at: 0.5, flip: false }
  const { container, rerender } = render(<SectionBar section={cut} onSection={() => {}} closed={false} />)
  expect(container.textContent).toContain(strings.section.open)
  rerender(<SectionBar section={cut} onSection={() => {}} closed={null} />)
  expect(container.textContent).toContain(strings.section.unknown)
  rerender(<SectionBar section={cut} onSection={() => {}} closed />)
  expect(container.textContent).not.toContain(strings.section.open)
  expect(container.textContent).not.toContain(strings.section.unknown)
  rerender(<SectionBar section={null} onSection={() => {}} closed={false} />)
  expect(container.textContent).not.toContain(strings.section.open)
})

/** A distance between moved parts would describe no shape the assembly has, so the tools wait until it is back together. */
test('measuring is off while the parts are apart, and the bar says why', () => {
  const onTool = vi.fn()
  render(<MeasureBar tool={null} onTool={onTool} reading={null} note={null} off={strings.explode.measuringOff} />)
  expect(screen.getByText(strings.explode.measuringOff)).toBeTruthy()
  const toolbar = screen.getByRole('toolbar', { name: strings.measure.label })
  const diameter = within(toolbar).getByRole('button', { name: strings.measure.tools.diameter }) as HTMLButtonElement
  expect(diameter.disabled).toBe(true)
  fireEvent.click(diameter)
  expect(onTool).not.toHaveBeenCalled()
})

test('the explode slider hands back how far apart the parts are drawn', () => {
  const onAmount = vi.fn()
  render(<ExplodeBar amount={0} onAmount={onAmount} />)
  fireEvent.change(screen.getByLabelText(strings.explode.label), { target: { value: '500' } })
  expect(onAmount).toHaveBeenLastCalledWith(0.5)
})
