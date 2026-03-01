import { sveltekit } from '@sveltejs/kit/vite';
import { VitePWA } from 'vite-plugin-pwa';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit(),
		VitePWA({
			// The manifest is provided manually in static/manifest.json so the
			// plugin only needs to inject the service-worker.
			manifest: false,
			registerType: 'autoUpdate',
			workbox: {
				// Cache all build artefacts plus the API read responses.
				globPatterns: ['**/*.{js,css,html,ico,png,svg,woff2}'],
				runtimeCaching: [
					{
						// Optionally cache file-read responses for offline viewing.
						urlPattern: /^.*\/api\/fs\/read/,
						handler: 'NetworkFirst',
						options: {
							cacheName: 'api-cache',
							expiration: { maxEntries: 50, maxAgeSeconds: 60 * 60 * 24 },
						},
					},
				],
			},
		}),
	],
	bundle: {
		// Vite 8 Rolldown-powered bundling
		experimentalrolldown: true 
	},

	// During frontend-only development proxy API calls to the running Rust
	// backend so hot-reload works without rebuilding the binary.
	server: {
		proxy: {
			'/api': {
				target: 'http://localhost:8123',
				changeOrigin: true,
			},
		},
	},
});
