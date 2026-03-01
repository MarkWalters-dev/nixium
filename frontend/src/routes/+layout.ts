// Disable SSR and pre-rendering globally: this is a pure client-side SPA
// served by the Rust binary; there is no Node.js server.
export const ssr = false;
export const prerender = false;
