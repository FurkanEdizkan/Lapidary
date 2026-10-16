import { fireEvent, render, screen } from '@testing-library/react'
import { expect, test, vi } from 'vitest'
import { ImportLinkDialog } from './ImportLink'
import { strings } from '../lib/strings'

test('the dialog imports the trimmed address, and shows a refusal in the server’s words', () => {
  const onImport = vi.fn()
  const view = render(<ImportLinkDialog busy={false} note={null} onImport={onImport} onCancel={() => {}} />)
  const confirm = screen.getByRole('button', { name: strings.importLink.confirm }) as HTMLButtonElement
  expect(confirm.disabled, 'nothing to import yet').toBe(true)
  fireEvent.change(screen.getByLabelText(strings.importLink.label), {
    target: { value: '  https://www.printables.com/model/482910-motorcycle-throttle-stop ' },
  })
  fireEvent.click(confirm)
  expect(onImport).toHaveBeenCalledWith('https://www.printables.com/model/482910-motorcycle-throttle-stop')

  const refusal = 'MakerWorld keeps its models behind a browser check that a server cannot pass.'
  view.rerender(<ImportLinkDialog busy={false} note={refusal} onImport={onImport} onCancel={() => {}} />)
  expect(screen.getByRole('alert').textContent).toBe(refusal)
})
