import { screen } from '@testing-library/react'
import { expect } from 'vitest'

/**
 * Open a menu the way a browser does.
 *
 * The menus are native popovers. jsdom 27 hides a closed one — its default stylesheet carries
 * the popover rules, so a control inside is not found by role, which is also what a user
 * sees — but it implements neither `showPopover()` nor `popovertarget` invocation, so a click
 * on the button opens nothing here. This does the browser's half: it follows the button's own
 * `popovertarget` to the menu and lifts the closed state off it.
 *
 * Following the attribute rather than finding the menu by id is the point. A menu whose
 * button is wired to nothing fails every test that opens it, instead of every test passing
 * against controls nobody can reach.
 */
export async function openMenu(label: string): Promise<HTMLElement> {
  const trigger = await screen.findByRole('button', { name: label })
  const target = trigger.getAttribute('popovertarget')
  expect(target, `the ${label} button opens no popover`).not.toBeNull()
  const menu = document.getElementById(target ?? '')
  expect(menu?.hasAttribute('popover'), `popovertarget="${target}" names no popover`).toBe(true)
  menu?.removeAttribute('popover')
  return menu as HTMLElement
}
