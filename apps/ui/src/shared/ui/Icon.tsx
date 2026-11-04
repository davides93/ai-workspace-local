// Icons: 1.6px stroke on a 20px grid.
const paths = {
  chat: "M4 5h12v8H9l-4 3v-3H4z",
  agent: "M10 3a3 3 0 1 1 0 6 3 3 0 0 1 0-6zM4 17c0-3 2.7-5 6-5s6 2 6 5",
  flow: "M5 4h4v4H5zM11 12h4v4h-4zM7 8v4h4",
  chip: "M6 6h8v8H6zM8 3v3M12 3v3M8 14v3M12 14v3M3 8h3M3 12h3M14 8h3M14 12h3",
  cube: "M10 3l6 3.5v7L10 17l-6-3.5v-7zM4 6.5l6 3.5 6-3.5M10 10v7",
  device: "M3 5h14v9H3zM7 17h6M10 14v3",
  tool: "M13 3a4 4 0 0 0-3.6 5.6L4 14l2 2 5.4-5.4A4 4 0 0 0 17 7l-2.5 2.5-2-2L15 5a4 4 0 0 0-2-2z",
  activity: "M3 10h3l2-5 4 10 2-5h3",
  settings: "M10 7a3 3 0 1 1 0 6 3 3 0 0 1 0-6zM10 2v2M10 16v2M2 10h2M16 10h2M4.3 4.3l1.4 1.4M14.3 14.3l1.4 1.4M4.3 15.7l1.4-1.4M14.3 5.7l1.4-1.4",
  plus: "M10 4v12M4 10h12",
  send: "M4 10l12-6-5 13-2-5z",
  stop: "M6 6h8v8H6z",
  power: "M10 3v7M6.2 5.8a6 6 0 1 0 7.6 0",
  play: "M7 5l8 5-8 5z",
  download: "M10 3v10M6 9l4 4 4-4M4 16h12",
  close: "M5 5l10 10M15 5L5 15",
  check: "M4 10l4 4 8-8",
  more: "M5 10h.01M10 10h.01M15 10h.01",
  panel: "M3 4h14v12H3zM12 4v12",
  list: "M4 6h12M4 10h12M4 14h12",
  shield: "M10 3l6 2v5c0 4-3 6-6 7-3-1-6-3-6-7V5z",
  link: "M8 12l4-4M7 9l-2 2a3 3 0 0 0 4 4l2-2M13 11l2-2a3 3 0 0 0-4-4l-2 2",
  trash: "M4 6h12M8 6V4h4v2M6 6l1 11h6l1-11",
  sparkle: "M10 3l1.6 4.4L16 9l-4.4 1.6L10 15l-1.6-4.4L4 9l4.4-1.6z",
  globe: "M10 3a7 7 0 1 1 0 14 7 7 0 0 1 0-14zM3 10h14M10 3c2 2 2.8 4.4 2.8 7S12 15 10 17c-2-2-2.8-4.4-2.8-7S8 5 10 3z",
  terminal: "M3 4h14v12H3zM6 8l3 2-3 2M10 13h4",
  file: "M5 3h7l3 3v11H5zM12 3v3h3",
  mouse: "M10 3a4 4 0 0 1 4 4v6a4 4 0 0 1-8 0V7a4 4 0 0 1 4-4zM10 3v5",
  refresh: "M15 6a6 6 0 1 0 1 5M15 3v3h-3",
  bolt: "M11 2L4 11h5l-1 7 7-9h-5z",
  branch: "M6 3v14M6 13a4 4 0 0 0 4-4h1a3 3 0 0 0 3-3M14 3a2 2 0 1 1 0 4 2 2 0 0 1 0-4zM6 15a2 2 0 1 1 0 4",
  home: "M3 9l7-6 7 6v8H3zM8 17v-5h4v5",
  plug: "M7 3v4M13 3v4M5 7h10v3a5 5 0 0 1-10 0zM10 15v3",
  search: "M9 3a6 6 0 1 1 0 12A6 6 0 0 1 9 3zM14 14l4 4",
  copy: "M7 7h9v10H7zM4 13V3h9",
  back: "M12 4l-6 6 6 6",
  forward: "M8 4l6 6-6 6",
  down: "M4 7l6 6 6-6",
  external: "M8 4H4v12h12v-4M11 3h6v6M17 3l-8 8",
  lock: "M5 9h10v8H5zM7 9V6a3 3 0 0 1 6 0v3",
  sun: "M10 6.5a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7zM10 2v2M10 16v2M2 10h2M16 10h2M4.3 4.3l1.4 1.4M14.3 14.3l1.4 1.4M4.3 15.7l1.4-1.4M14.3 5.7l1.4-1.4",
  moon: "M16 12.5A7 7 0 0 1 7.5 4 7 7 0 1 0 16 12.5z",
  alert: "M10 3l8 14H2zM10 8v4M10 14.5h.01",
  info: "M10 3a7 7 0 1 1 0 14 7 7 0 0 1 0-14zM10 9v5M10 6.5h.01",
  key: "M13 8a3 3 0 1 1-6 0 3 3 0 0 1 6 0zM10 11v6M10 14h3",
  edit: "M4 16l1-4 8-8 3 3-8 8zM12 5l3 3",
  mic: "M10 3a2.5 2.5 0 0 1 2.5 2.5v4a2.5 2.5 0 0 1-5 0v-4A2.5 2.5 0 0 1 10 3zM5.5 9.5a4.5 4.5 0 0 0 9 0M10 14v3M7.5 17h5",
  wave: "M3 9v2M6 6.5v7M9 4v12M12 7v6M15 5.5v9M17.5 9v2",
  cloud: "M6 15a4 4 0 0 1-.5-8 5 5 0 0 1 9.5 1A3.5 3.5 0 0 1 14.5 15z",
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 18, className }: { name: IconName | (string & {}); size?: number; className?: string }) {
  const d = (paths as Record<string, string>)[name] ?? paths.more;
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className={className}>
      <path d={d} />
    </svg>
  );
}
