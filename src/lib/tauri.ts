/**
 * Runtime environment helpers. The same React app runs in the Tauri webview and
 * in a browser served by the app's local web server; these decide which
 * transport (Tauri IPC vs. `fetch` to `/api/invoke`) to use.
 */

/** True when running inside the Tauri webview rather than a plain browser. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** The token for the local web API, captured from the URL on first load. */
export function webToken(): string {
  if (typeof window === "undefined") return "";
  const fromUrl = new URLSearchParams(window.location.search).get("token");
  if (fromUrl) {
    window.sessionStorage.setItem("orbit.webToken", fromUrl);
    return fromUrl;
  }
  return window.sessionStorage.getItem("orbit.webToken") ?? "";
}
