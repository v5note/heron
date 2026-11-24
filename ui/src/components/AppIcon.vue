<script setup lang="ts">
// Inline 24px stroke icons (no icon font, no SVG sprite file): one path set shared by the rail,
// the toolbar and table actions.
const props = withDefaults(defineProps<{ name: IconName; size?: number }>(), { size: 15 });
export type IconName =
  | "overview" | "library" | "discover" | "sources" | "projects" | "upgrades" | "doctor" | "settings"
  | "sun" | "moon" | "auto" | "search" | "refresh" | "check" | "close" | "plus" | "external" | "alert" | "chevron" | "copy"
  | "grid" | "list" | "globe" | "spark" | "terminal" | "pointer" | "braces" | "home"
  | "robot" | "rocket" | "star" | "cube" | "bolt" | "code" | "sparkles" | "trash";

const PATHS: Record<IconName, string> = {
  overview: '<path d="M3 12h4l2.5-6 4 12 2.5-6H21"/>',
  library: '<rect x="3.5" y="4" width="4" height="16" rx="1"/><rect x="9.5" y="4" width="4" height="16" rx="1"/><path d="M15.5 5.2l3.6 14.6"/>',
  discover: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>',
  sources: '<path d="M4 7h16M4 12h10M4 17h13"/><circle cx="18.5" cy="16.5" r="2.5"/>',
  projects: '<path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2h8a1.5 1.5 0 0 1 1.5 1.5V18A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18z"/>',
  upgrades: '<path d="M12 20V4"/><path d="M6.5 9.5 12 4l5.5 5.5"/>',
  doctor: '<path d="M3.5 12h4l1.5-4 3 8 2-4h6.5"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M12 3.5v2M12 18.5v2M3.5 12h2M18.5 12h2M6 6l1.4 1.4M16.6 16.6 18 18M6 18l1.4-1.4M16.6 7.4 18 6"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4"/>',
  moon: '<path d="M20 14.2A8 8 0 1 1 9.8 4a6.4 6.4 0 0 0 10.2 10.2z"/>',
  // Auto / follow the system: the letter A. The crossbar runs past both legs so it visually
  // connects (a bar ending inside the counters reads as detached), and round caps soften the apex.
  auto: '<path d="M6 19 12 5l6 14" stroke-linecap="round" stroke-linejoin="round"/><path d="M7.6 14.5h8.8" stroke-linecap="round"/>',
  search: '<circle cx="11" cy="11" r="6"/><path d="M15.5 15.5 20 20"/>',
  refresh: '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4v4h-4"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  external: '<path d="M14 4h6v6"/><path d="M20 4 11 13"/><path d="M18 14v5.5H4.5V6H10"/>',
  alert: '<path d="M12 4 21 20H3z"/><path d="M12 10v4"/>',
  chevron: '<path d="M9 6l6 6-6 6"/>',
  copy: '<rect x="9" y="9" width="11.5" height="11.5" rx="2.5"/><path d="M5.5 15.5A1.5 1.5 0 0 1 4 14V5.5A1.5 1.5 0 0 1 5.5 4H14a1.5 1.5 0 0 1 1.5 1.5"/>',
  // Source clients. `agents` is the client-agnostic shared location, so it gets a globe.
  grid: '<rect x="4" y="4" width="7" height="7" rx="1.6"/><rect x="13" y="4" width="7" height="7" rx="1.6"/><rect x="4" y="13" width="7" height="7" rx="1.6"/><rect x="13" y="13" width="7" height="7" rx="1.6"/>',
  list: '<path d="M4.5 6.5h15M4.5 12h15M4.5 17.5h10"/>',
  globe: '<circle cx="12" cy="12" r="8"/><path d="M4 12h16"/><path d="M12 4c2.4 2.2 2.4 13.8 0 16"/><path d="M12 4c-2.4 2.2-2.4 13.8 0 16"/>',
  spark: '<path d="M12 3.5v17M4.6 7.75l14.8 8.5M19.4 7.75l-14.8 8.5"/>',
  terminal: '<path d="M5 7l5 5-5 5"/><path d="M12 17h7"/>',
  pointer: '<path d="M6 3.5 19 12l-6.2 1.3L9.6 20z"/>',
  braces: '<path d="M9.5 4.5c-1.8 0-2.8.9-2.8 2.9v2.1c0 1.3-.7 2.1-2.2 2.5 1.5.4 2.2 1.2 2.2 2.5v2.1c0 2 1 2.9 2.8 2.9"/><path d="M14.5 4.5c1.8 0 2.8.9 2.8 2.9v2.1c0 1.3.7 2.1 2.2 2.5-1.5.4-2.2 1.2-2.2 2.5v2.1c0 2-1 2.9-2.8 2.9"/>',
  home: '<path d="M4 11.5 12 4l8 7.5"/><path d="M6.5 10.5V20h11v-9.5"/>',
  robot: '<rect x="5" y="8" width="14" height="11" rx="3"/><path d="M12 4.5V8"/><circle cx="12" cy="3.6" r="1"/><circle cx="9.5" cy="13" r="1.1"/><circle cx="14.5" cy="13" r="1.1"/><path d="M9.5 16.5h5"/>',
  rocket: '<path d="M12 3.5c3 2.6 4.3 5.7 4.3 9.2L12 17.5l-4.3-4.8c0-3.5 1.3-6.6 4.3-9.2z"/><circle cx="12" cy="9.8" r="1.6"/><path d="M7.7 14.5 6 19l3.8-1.4M16.3 14.5 18 19l-3.8-1.4"/>',
  star: '<path d="M12 4l2.4 5.1 5.4.6-4.1 3.8 1.2 5.5-4.9-2.9-4.9 2.9 1.2-5.5-4.1-3.8 5.4-.6z"/>',
  cube: '<path d="M12 4l7 3.9v8.2L12 20l-7-3.9V7.9z"/><path d="M5 7.9 12 12l7-4.1M12 12v8"/>',
  bolt: '<path d="M13 4 6.5 13.5H11L10 20l6.5-9.5H12z"/>',
  code: '<path d="M8.5 8.5 4.5 12l4 3.5M15.5 8.5 19.5 12l-4 3.5"/>',
  sparkles: '<path d="M11 4.5c.6 3 1.4 3.8 4.4 4.4-3 .6-3.8 1.4-4.4 4.4-.6-3-1.4-3.8-4.4-4.4 3-.6 3.8-1.4 4.4-4.4z"/><path d="M17 14c.4 1.8.9 2.3 2.7 2.7-1.8.4-2.3.9-2.7 2.7-.4-1.8-.9-2.3-2.7-2.7 1.8-.4 2.3-.9 2.7-2.7z"/>',
  trash: '<path d="M4.5 7h15"/><path d="M9.5 7V4.5h5V7"/><path d="M6.5 7l1 12.5h9l1-12.5"/><path d="M10 10.5v6M14 10.5v6"/>',
};
</script>

<template>
  <svg :width="props.size" :height="props.size" viewBox="0 0 24 24" aria-hidden="true" v-html="PATHS[props.name]" />
</template>
