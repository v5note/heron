import { createRouter, createWebHashHistory } from "vue-router";

// Hash history: the Tauri shell loads the app from a file path.
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "overview", component: () => import("./views/OverviewView.vue") },
    { path: "/library", name: "library", component: () => import("./views/LibraryView.vue") },
    { path: "/discover", name: "discover", component: () => import("./views/DiscoverView.vue") },
    { path: "/favorites", name: "favorites", component: () => import("./views/FavoritesView.vue") },
    { path: "/sources", name: "sources", component: () => import("./views/SourcesView.vue") },
    { path: "/projects", name: "projects", component: () => import("./views/ProjectsView.vue") },
    { path: "/upgrades", name: "upgrades", component: () => import("./views/UpgradesView.vue") },
    { path: "/doctor", name: "doctor", component: () => import("./views/DoctorView.vue") },
    { path: "/settings", name: "settings", component: () => import("./views/SettingsView.vue") },
  ],
});
