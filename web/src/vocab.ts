import { AfterMode } from './api'

/** One name per concept, used by the cards, the Start strip, the summaries and the guide. */
export const AFTER_MODE_NAME: Record<AfterMode, string> = {
  video: 'Keep video',
  video_stack: 'Video + stack',
  stack_only: 'Stack only',
  best_frames: 'Best frames + stack',
}

export type HoldLevel = 'locked' | 'settling' | 'off'
/** The guiding error as a word, so the number is never the only signal: a few pixels is locked, a few tens still settling. */
export function holdLevel(errorPx: number | null | undefined): HoldLevel {
  return errorPx == null ? 'settling' : errorPx <= 10 ? 'locked' : errorPx <= 60 ? 'settling' : 'off'
}
export const HOLD_WORD: Record<HoldLevel, string> = { locked: 'Locked', settling: 'Settling', off: 'Off target' }
