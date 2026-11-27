import type { RouteLocationNormalizedLoaded } from "vue-router";

/** Extract a single string `?q=` search seed from the current route, or empty string. */
export function routeSearchSeed(route: RouteLocationNormalizedLoaded): string {
  return typeof route.query.q === "string" ? route.query.q : "";
}
