<script lang="ts">
	import type { ExtensionManifest } from '$lib/extensions';

	export interface StoreEntry {
		name: string; displayName: string; version: string;
		description: string; author?: string; download_url: string;
		readme_url?: string;
	}

	let {
		extList,
		enabledExtensions,
		extDetailName,
		installingExt = null,
		storeRegistry = $bindable([]),
		onclose,
		onopendetail,
		onopenstoredetail,
		ontoggle,
		oninstall,
	}: {
		extList: ExtensionManifest[];
		enabledExtensions: Record<string, boolean>;
		extDetailName: string | null;
		installingExt?: string | null;
		storeRegistry?: StoreEntry[];
		onclose: () => void;
		onopendetail: (name: string) => void;
		onopenstoredetail: (entry: StoreEntry) => void;
		ontoggle: (name: string, enabled: boolean) => void;
		oninstall: (entry: StoreEntry) => void;
	} = $props();

	let extTab       = $state<'installed' | 'store'>('installed');
	let storeQuery   = $state('');
	let storeLoading = $state(false);
	let storeError   = $state('');

	const storeResults = $derived.by(() => {
		const q = storeQuery.trim().toLowerCase();
		if (!q) return storeRegistry;
		return storeRegistry.filter((e: StoreEntry) =>
			e.name.toLowerCase().includes(q) ||
			e.displayName.toLowerCase().includes(q) ||
			(e.description?.toLowerCase().includes(q) ?? false) ||
			(e.author?.toLowerCase().includes(q) ?? false)
		);
	});

	export async function loadStore() {
		if (storeRegistry.length > 0 || storeLoading) return;
		storeLoading = true; storeError = '';
		try {
			const res = await fetch('/api/extensions/store/search?q=');
			if (res.ok) {
				const remote: StoreEntry[] = await res.json();
				const updated = new Map(storeRegistry.map((e: StoreEntry) => [e.name, e]));
				for (const e of remote) updated.set(e.name, e);
				storeRegistry = [...updated.values()];
			} else storeError = `Store unavailable (${res.status})`;
		} catch (e) { storeError = (e as Error).message; }
		finally { storeLoading = false; }
	}

	$effect(() => {
		// Seed registry with installed extensions not yet in store results
		const known = new Set(storeRegistry.map((e: StoreEntry) => e.name));
		const newEntries: StoreEntry[] = extList
			.filter(e => !known.has(e.name))
			.map(e => ({ name: e.name, displayName: e.displayName, version: e.version, description: e.description, download_url: '' }));
		if (newEntries.length) storeRegistry = [...storeRegistry, ...newEntries];
	});
</script>

<div class="ext-panel">
	<div class="ext-header">
		<span class="ext-title">Extensions</span>
		<button class="icon-btn" onclick={onclose} title="Close">×</button>
	</div>
	<div class="ext-tabs">
		<button class="ext-tab" class:ext-tab-active={extTab === 'installed'} onclick={() => extTab = 'installed'}>Installed</button>
		<button class="ext-tab" class:ext-tab-active={extTab === 'store'}
			onclick={() => { extTab = 'store'; loadStore(); }}>Store</button>
	</div>

	{#if extTab === 'installed'}
		{#if extList.length === 0}
			<div class="ext-empty">
				<p>No extensions installed.</p>
				<p class="ext-empty-hint">Install from the Store tab, or drop extensions here:</p>
				<code class="ext-empty-path">~/.config/nixium/extensions/</code>
				<p class="ext-empty-hint">Each needs a <code>manifest.json</code> and an <code>index.js</code>.</p>
			</div>
		{:else}
			{#each extList as ext}
				<div class="ext-item" class:ext-item-selected={extDetailName === ext.name}>
					<button class="ext-item-btn" onclick={() => onopendetail(ext.name)}>
						<span class="ext-item-label">{ext.displayName}</span>
						<span class="ext-item-desc">v{ext.version} — {ext.description}</span>
					</button>
					<div class="ext-item-actions">
						<input
							type="checkbox"
							checked={enabledExtensions[ext.name] ?? false}
							onchange={(e) => ontoggle(ext.name, (e.target as HTMLInputElement).checked)}
							class="ext-toggle"
							title={enabledExtensions[ext.name] ? 'Disable' : 'Enable'}
						/>
					</div>
				</div>
			{/each}
		{/if}
	{:else}
		<div class="ext-store-search">
			<input type="search" class="ext-store-input" placeholder="Search extensions…"
				bind:value={storeQuery}
				onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); e.stopPropagation(); loadStore(); } }} />
			<button class="ext-store-btn" onclick={loadStore} disabled={storeLoading}>{storeLoading ? '…' : '⌕'}</button>
		</div>
		{#if storeError}
			<div class="ext-store-msg ext-store-err">{storeError}</div>
		{:else if storeLoading}
			<div class="ext-store-msg">Searching…</div>
		{:else if storeResults.length === 0 && storeRegistry.length > 0}
			<div class="ext-store-msg">No results for "{storeQuery}".</div>
		{:else if storeResults.length === 0}
			<div class="ext-store-msg">No extensions found.</div>
		{:else}
			{#each storeResults as entry}
				{@const alreadyInstalled = extList.some(e => e.name === entry.name)}
				<div class="ext-store-item" class:ext-item-selected={extDetailName === entry.name}>
					<button class="ext-item-btn" onclick={() => alreadyInstalled ? onopendetail(entry.name) : onopenstoredetail(entry)}>
						<span class="ext-item-label">{entry.displayName}</span>
						<span class="ext-item-desc">v{entry.version}{entry.author ? ` · ${entry.author}` : ''}</span>
						{#if entry.description}<span class="ext-store-desc">{entry.description}</span>{/if}
					</button>
					{#if alreadyInstalled}
						<span class="ext-store-badge">✓</span>
					{:else}
						<button class="ext-store-install-btn"
							onclick={() => oninstall(entry)}
							disabled={installingExt === entry.name}
						>{ installingExt === entry.name ? '…' : 'Install' }</button>
					{/if}
				</div>
			{/each}
		{/if}
	{/if}
</div>

<style>
	.ext-panel { display: flex; flex-direction: column; height: 100%; overflow-y: auto; }
	.ext-header { display: flex; align-items: center; justify-content: space-between; padding: 6px 8px 4px; flex: 0 0 auto; border-bottom: 1px solid var(--border); }
	.ext-title { font-size: 10px; font-weight: 700; letter-spacing: .07em; text-transform: uppercase; color: var(--muted); }
	.icon-btn { flex: 0 0 auto; background: none; border: none; cursor: pointer; color: var(--muted); font-size: 15px; padding: 1px 4px; border-radius: var(--radius); line-height: 1; transition: color .12s, background .12s; }
	.icon-btn:hover { color: var(--text); background: var(--hover-bg); }
	.ext-item { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: 8px 10px; border-bottom: 1px solid var(--border); }
	.ext-item:hover { background: var(--hover-bg); }
	.ext-item-label { font-size: 12px; color: var(--text); }
	.ext-item-desc { font-size: 10px; color: var(--muted); }
	.ext-toggle { flex: 0 0 auto; width: 16px; height: 16px; accent-color: var(--accent); cursor: pointer; }
	.ext-empty { padding: 16px 12px; display: flex; flex-direction: column; gap: 6px; }
	.ext-empty p { margin: 0; font-size: 12px; color: var(--text); }
	.ext-empty-hint { font-size: 11px; color: var(--muted); }
	.ext-empty-path { display: block; font-family: 'JetBrains Mono', monospace; font-size: 11px; color: var(--accent); padding: 4px 6px; background: var(--surface); border-radius: 4px; word-break: break-all; margin-top: 2px; }
	.ext-item-btn { background: none; border: none; color: inherit; cursor: pointer; text-align: left; padding: 0; min-width: 0; flex: 1 1 0; display: flex; flex-direction: column; gap: 2px; }
	.ext-item-btn:hover .ext-item-label { color: var(--accent); }
	.ext-item-selected { background: color-mix(in srgb, var(--accent) 10%, transparent); }
	.ext-tabs { display: flex; border-bottom: 1px solid var(--border); flex: 0 0 auto; }
	.ext-tab { flex: 1; background: none; border: none; border-bottom: 2px solid transparent; padding: 5px 0; font-size: 11px; color: var(--muted); cursor: pointer; transition: color .1s, border-color .1s; }
	.ext-tab:hover { color: var(--text); }
	.ext-tab-active { color: var(--accent) !important; border-bottom-color: var(--accent) !important; }
	.ext-item-actions { display: flex; align-items: center; gap: 4px; flex: 0 0 auto; }
	.ext-store-search { display: flex; gap: 4px; padding: 6px 8px; border-bottom: 1px solid var(--border); flex: 0 0 auto; }
	.ext-store-input { flex: 1; background: var(--surface); border: 1px solid var(--border); border-radius: 4px; color: var(--text); font-size: 12px; padding: 3px 6px; outline: none; }
	.ext-store-input:focus { border-color: var(--accent); }
	.ext-store-btn { background: var(--surface); border: 1px solid var(--border); border-radius: 4px; color: var(--text); font-size: 14px; padding: 2px 8px; cursor: pointer; }
	.ext-store-btn:hover:not(:disabled) { border-color: var(--accent); color: var(--accent); }
	.ext-store-btn:disabled { opacity: .4; cursor: default; }
	.ext-store-msg { font-size: 11px; color: var(--muted); padding: 10px 10px; }
	.ext-store-err { color: #f99; }
	.ext-store-item { display: flex; align-items: flex-start; justify-content: space-between; gap: 8px; padding: 8px 10px; border-bottom: 1px solid var(--border); }
	.ext-store-item:hover { background: var(--hover-bg); }
	.ext-store-desc { font-size: 10px; color: var(--muted); display: block; margin-top: 2px; line-height: 1.4; white-space: normal; }
	.ext-store-install-btn { flex: 0 0 auto; font-size: 11px; padding: 3px 8px; background: var(--accent); color: #000; border: none; border-radius: 4px; cursor: pointer; font-weight: 600; }
	.ext-store-install-btn:hover:not(:disabled) { opacity: .85; }
	.ext-store-install-btn:disabled { opacity: .45; cursor: default; }
	.ext-store-badge { flex: 0 0 auto; font-size: 11px; color: #6f9; padding: 2px 6px; }
</style>
