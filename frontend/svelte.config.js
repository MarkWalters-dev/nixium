import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
	preprocess: vitePreprocess(),

	kit: {
		adapter: adapter({
			// Every unmatched route falls back to index.html so the SvelteKit
			// client-side router handles navigation without a server.
			fallback: 'index.html',
			// Output directory consumed by rust-embed (relative to frontend/).
			pages: 'build',
			assets: 'build',
			strict: false,
		}),
	},
};

export default config;
