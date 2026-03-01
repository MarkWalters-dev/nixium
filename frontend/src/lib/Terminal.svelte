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

	export function focus() { term?.focus(); }
	export function sendText(text: string) {
		if (ws?.readyState === WebSocket.OPEN) ws.send(text);
	}

	onMount(() => {
		term = new Terminal({
cursorBlink: true,
fontSize: 13,
fontFamily: "'JetBrains Mono', 'Cascadia Code', 'Fira Code', monospace",
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

		const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const cwdParam = cwd ? `?cwd=${encodeURIComponent(cwd)}` : '';
		ws = new WebSocket(`${proto}//${location.host}/api/terminal/ws${cwdParam}`);
		ws.binaryType = 'arraybuffer';

		ws.addEventListener('open', () => sendResize());
		ws.addEventListener('message', (ev) => {
			if (ev.data instanceof ArrayBuffer) {
				term.write(new Uint8Array(ev.data));
			} else {
				term.write(ev.data as string);
			}
		});
		ws.addEventListener('close', () => term.writeln('\r\n\x1b[31m[connection closed]\x1b[0m'));
		ws.addEventListener('error', () => term.writeln('\r\n\x1b[31m[websocket error]\x1b[0m'));

		term.onData((data) => { if (ws?.readyState === WebSocket.OPEN) ws.send(data); });

		resizeObs = new ResizeObserver(() => { fitAddon.fit(); sendResize(); });
		resizeObs.observe(container!);

		return () => {
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
