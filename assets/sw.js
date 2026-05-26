const VERSION = "__CACHE_VERSION__"; // replaced at build time with the sha256 of app_bg.wasm
const APP_NAME = "egui-screensaver-mystify";
const CACHE_NAME = `${APP_NAME}-${VERSION}`;
const APP_STATIC_RESOURCES = [
  "./",
  "./index.html",
  "./app.js",
  "./app_bg.wasm",
  "./manifest.json",
  "./icons/512.png",
  "./icons/192.png",
];

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      await cache.addAll(APP_STATIC_RESOURCES);
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(
        names.map((name) => {
          if (name.startsWith(APP_NAME) && name !== CACHE_NAME) {
            return caches.delete(name);
          }
          return Promise.resolve(false);
        }),
      );
      await clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;

  event.respondWith(
    (async () => {
      const cache = await caches.open(CACHE_NAME);
      if (event.request.mode === "navigate") {
        try {
          return await fetch(event.request);
        } catch {
          const cachedIndex = await cache.match(new URL("./index.html", self.location).href);
          if (cachedIndex) return cachedIndex;
          throw new Error("Offline and no cached index.html");
        }
      }
      const cachedResponse = await cache.match(event.request);
      if (cachedResponse) return cachedResponse;

      try {
        const networkResponse = await fetch(event.request);
        const url = new URL(event.request.url);
        if (url.origin === self.location.origin && networkResponse.ok) {
          await cache.put(event.request, networkResponse.clone());
        }
        return networkResponse;
      } catch {
        const fallback = await cache.match(event.request, { ignoreSearch: true });
        if (fallback) return fallback;
        throw new Error(`Offline and no cache match: ${event.request.url}`);
      }
    })(),
  );
});
