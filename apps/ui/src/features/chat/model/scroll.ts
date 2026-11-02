export interface ScrollMetrics {
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

/** Pixels hidden below the visible part of a scroll container. */
export const distanceFromBottom = (m: ScrollMetrics): number => Math.max(0, m.scrollHeight - m.scrollTop - m.clientHeight);

/** Is the reader at (or within `threshold` px of) the bottom? Only then does new content pull the view along. */
export const isNearBottom = (m: ScrollMetrics, threshold = 64): boolean => distanceFromBottom(m) <= threshold;
