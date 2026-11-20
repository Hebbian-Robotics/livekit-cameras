import { useSyncExternalStore } from "react";

const mobileViewport = window.matchMedia("(width < 48rem)");

function subscribeToViewport(onChange: () => void) {
  mobileViewport.addEventListener("change", onChange);
  return () => mobileViewport.removeEventListener("change", onChange);
}

function isMobileViewport() {
  return mobileViewport.matches;
}

export function useIsMobile() {
  return useSyncExternalStore(subscribeToViewport, isMobileViewport);
}
