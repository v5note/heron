<script setup lang="ts">
// Inline 24px stroke icons (no icon font, no SVG sprite file): one path set shared by the rail,
// the toolbar and table actions.
const props = withDefaults(defineProps<{ name: IconName; size?: number }>(), { size: 15 });
export type IconName =
  | "overview" | "library" | "discover" | "sources" | "projects" | "upgrades" | "doctor" | "settings"
  | "sun" | "moon" | "search" | "refresh" | "check" | "close" | "plus" | "external" | "alert" | "chevron";

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
  search: '<circle cx="11" cy="11" r="6"/><path d="M15.5 15.5 20 20"/>',
  refresh: '<path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3"/><path d="M19.5 4v4h-4"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7"/>',
  close: '<path d="M6 6l12 12M18 6 6 18"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  external: '<path d="M14 4h6v6"/><path d="M20 4 11 13"/><path d="M18 14v5.5H4.5V6H10"/>',
  alert: '<path d="M12 4 21 20H3z"/><path d="M12 10v4"/>',
  chevron: '<path d="M9 6l6 6-6 6"/>',
};
</script>

<template>
  <svg :width="props.size" :height="props.size" viewBox="0 0 24 24" aria-hidden="true" v-html="PATHS[props.name]" />
</template>
