<script lang="ts">
	import { tick } from 'svelte';
	import type { PaletteCommand } from '$lib/types';

	let {
		commands,
		onrun,
		onclose,
		onjumpline,
	}: {
		commands: PaletteCommand[];
		onrun: (cmd: PaletteCommand) => void;
		onclose: () => void;
		onjumpline: (line: number) => void;
	} = $props();

	let query          = $state('');
	let selectedIdx    = $state(0);
	let inputEl        = $state<HTMLInputElement | null>(null);

	$effect(() => {
		tick().then(() => inputEl?.focus());
	});

	$effect(() => {
		// Reset selection when query changes
		// eslint-disable-next-line @typescript-eslint/no-unused-expressions
		query;
		selectedIdx = 0;
	});

	function _match(label: string, q: string): boolean {
		if (!q) return true;
		let i = 0;
		const l = label.toLowerCase();
		for (const ch of q) { const pos = l.indexOf(ch, i); if (pos === -1) return false; i = pos + 1; }
		return true;
	}

	const filtered = $derived((): PaletteCommand[] => {
		const q = query.trim().toLowerCase();
		if (q.startsWith(':')) {
			const n = parseInt(q.slice(1), 10);
			if (!isNaN(n) && n > 0) {
				return [{ id: 'gotoline', label: `Go to Line ${n}`, description: `Jump to line ${n} in current file`, action: () => onjumpline(n) }];
			}
			return [];
		}
		return commands.filter(c => _match(c.label, q));
	});

	function run(cmd: PaletteCommand) {
		onclose();
		cmd.action();
		onrun(cmd);
	}

	function onkeydown(e: KeyboardEvent) {
		const items = filtered();
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			selectedIdx = Math.min(selectedIdx + 1, items.length - 1);
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			selectedIdx = Math.max(selectedIdx - 1, 0);
		} else if (e.key === 'Enter') {
			e.preventDefault();
			const cmd = items[selectedIdx];
			if (cmd) run(cmd);
		} else if (e.key === 'Escape') {
			e.preventDefault();
			onclose();
		}
	}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="palette-backdrop" role="presentation" onmousedown={onclose}>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div class="palette" role="dialog" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
		<div class="palette-search">
			<span class="palette-icon">⌘</span>
			<input
				class="palette-input"
				placeholder="Type a command or ':42' to go to line…"
				bind:value={query}
				bind:this={inputEl}
				{onkeydown}
				autocomplete="off"
				spellcheck={false}
			/>
		</div>
		{#if filtered().length > 0}
			<ul class="palette-list" role="listbox">
				{#each filtered() as cmd, i}
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<li
						role="option"
						aria-selected={i === selectedIdx}
						class="palette-item"
						class:palette-item-active={i === selectedIdx}
						onmouseenter={() => (selectedIdx = i)}
						onclick={() => run(cmd)}
					>
						<span class="palette-label">{cmd.label}</span>
						{#if cmd.description}<span class="palette-desc">{cmd.description}</span>{/if}
						{#if cmd.keybinding}<span class="palette-kbd">{cmd.keybinding}</span>{/if}
					</li>
				{/each}
			</ul>
		{:else}
			<div class="palette-empty">No matching commands</div>
		{/if}
	</div>
</div>

<style>
	.palette-backdrop { position: fixed; inset: 0; z-index: 900; background: #00000055; display: flex; justify-content: center; align-items: flex-start; padding-top: 10vh; }
	.palette { width: min(600px, 92vw); background: var(--surface); border: 1px solid var(--border); border-radius: 8px; box-shadow: 0 24px 64px #000000bb; display: flex; flex-direction: column; overflow: hidden; }
	.palette-search { display: flex; align-items: center; gap: 8px; padding: 10px 12px; border-bottom: 1px solid var(--border); }
	.palette-icon { font-size: 14px; color: var(--muted); flex: 0 0 auto; }
	.palette-input { flex: 1; background: none; border: none; outline: none; color: var(--text); font-size: 14px; font-family: system-ui, sans-serif; }
	.palette-input::placeholder { color: var(--muted); }
	.palette-list { list-style: none; max-height: 380px; overflow-y: auto; padding: 4px 0; }
	.palette-item { display: flex; align-items: center; gap: 8px; padding: 7px 14px; cursor: pointer; }
	.palette-item:hover, .palette-item-active { background: var(--hover-bg); }
	.palette-item-active { background: var(--active-bg) !important; }
	.palette-label { flex: 1 1 auto; font-size: 13px; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.palette-desc { font-size: 11px; color: var(--muted); flex: 0 0 auto; }
	.palette-kbd { font-size: 10px; color: var(--muted); border: 1px solid var(--border); border-radius: 3px; padding: 1px 5px; white-space: nowrap; flex: 0 0 auto; font-family: 'JetBrains Mono', monospace; }
	.palette-empty { padding: 12px 14px; font-size: 12px; color: var(--muted); }
</style>
