/**
 * Motion's animation features, loaded on first use rather than in the entry chunk (A18). `domMax` because layout and
 * shared-element animation (`layoutId`) need it; `LazyMotion` in `motion-react.tsx` fetches this file when a motion
 * element first renders.
 */
export { domMax as default } from 'motion/react'
