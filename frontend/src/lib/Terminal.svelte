<script lang="ts">
	import { onMount } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import '@xterm/xterm/css/xterm.css';

	let { cwd = '' }: { cwd?: string } = $props();

	let container = $state<HTMLDivElement>();
	let term: Terminal;
	let fitAddon: FitAddon;
	let ws: WebSocket | null = null;
	let resizeObs: ResizeObserver;
	let destroyed = false;
	let retryTimer: ReturnType<typeof setTimeout> | null = null;
	let retryDelay = 1000; // ms, doubles each attempt up to 10s

	export function focus() { term?.focus(); }
	export function sendText(text: string) {
		if (ws?.readyState === WebSocket.OPEN) ws.send(text);
	}

	function connect() {
		if (destroyed) return;
		const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const cwdParam = cwd ? `?cwd=${encodeURIComponent(cwd)}` : '';
		ws = new WebSocket(`${proto}//${location.host}/api/terminal/ws${cwdParam}`);
		ws.binaryType = 'arraybuffer';

		ws.addEventListener('open', () => {
			retryDelay = 1000; // reset backoff on successful connect
			sendResize();
		});

		ws.addEventListener('message', (ev) => {
			if (ev.data instanceof ArrayBuffer) {
				term.write(new Uint8Array(ev.data));
			} else {
				term.write(ev.data as string);
			}
		});

		ws.addEventListener('close', () => {
			if (destroyed) return;
			term.writeln(`\r\n\x1b[33m[disconnected — reconnecting in ${retryDelay / 1000}s…]\x1b[0m`);
			retryTimer = setTimeout(() => {
				retryDelay = Math.min(retryDelay * 2, 10_000);
				connect();
			}, retryDelay);
		});

		ws.addEventListener('error', () => {
			// close event fires right after, which handles the retry
		});
	}

	onMount(() => {
		term = new Terminal({
cursorBlink: true,
fontSize: 13,
// Self-hosted Nerd Font is first — guaranteed to load regardless of system fonts.
fontFamily: "'FiraCode Nerd Font Mono', 'JetBrainsMono Nerd Font Mono', 'Fira Code', monospace",
unicodeVersion: '11',
theme: {
background: '#11111b', foreground: '#cdd6f4', cursor: '#f5c2e7',
selectionBackground: '#45475a',
black:'#45475a', brightBlack:'#585b70',
red:'#f38ba8', brightRed:'#f38ba8',
green:'#a6e3a1', brightGreen:'#a6e3a1',
yellow:'#f9e2af', brightYellow:'#f9e2af',
blue:'#89b4fa', brightBlue:'#89b4fa',
magenta:'#f5c2e7', brightMagenta:'#f5c2e7',
cyan:'#94e2d5', brightCyan:'#94e2d5',
white:'#bac2de', brightWhite:'#a6adc8',
},
});
		fitAddon = new FitAddon();
		term.loadAddon(fitAddon);
		term.open(container!);
		fitAddon.fit();

		connect();

		term.onData((data) => { if (ws?.readyState === WebSocket.OPEN) ws.send(data); });

		resizeObs = new ResizeObserver(() => { fitAddon.fit(); sendResize(); });
		resizeObs.observe(container!);

		return () => {
			destroyed = true;
			if (retryTimer) clearTimeout(retryTimer);
			resizeObs?.disconnect();
			ws?.close();
			term?.dispose();
		};
	});

	function sendResize() {
		if (!term || !ws || ws.readyState !== WebSocket.OPEN) return;
		ws.send(`\x00resize:${term.cols}:${term.rows}`);
	}
</script>

<div class="terminal-wrap" bind:this={container}></div>

<style>
	.terminal-wrap {
		width: 100%;
		height: 100%;
		overflow: hidden;
		background: #11111b;
	}
	:global(.terminal-wrap .xterm-viewport) { overflow-y: auto !important; }
</style>
