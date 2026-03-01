<script lang="ts">
	import { untrack } from 'svelte';

	interface FsEntry { name: string; path: string; is_dir: boolean; }

	let {
		initialPath = '/',
		onselect,
		oncancel,
	}: {
		initialPath?: string;
		onselect: (path: string) => void;
		oncancel: () => void;
	} = $props();

	let currentPath = $state(untrack(() => initialPath || '/'));
	let entries     = $state<FsEntry[]>([]);
	let loading     = $state(false);
	let error       = $state('');
	let selected    = $state(untrack(() => currentPath)); // highlight current dir

	$effect(() => {
		const p = currentPath;
		untrack(() => loadDir(p));
	});

	async function loadDir(path: string) {
		loading = true;
		error = '';
		try {
			const res = await fetch(`/api/fs/list?path=${encodeURIComponent(path)}`);
			if (!res.ok) {
				const body = await res.json().catch(() => ({ error: res.statusText }));
				throw new Error(body.error ?? res.statusText);
			}
			const all: FsEntry[] = await res.json();
			entries = all.filter((e) => e.is_dir).sort((a, b) => a.name.localeCompare(b.name));
			selected = path;
			currentPath = path;
		} catch (err) {
			error = (err as Error).message;
		} finally {
			loading = false;
		}
	}

	function goUp() {
		const parts = currentPath.replace(/\/+$/, '').split('/').filter(Boolean);
		if (parts.length === 0) return;
		parts.pop();
		currentPath = '/' + parts.join('/');
	}

	function navigate(path: string) {
		currentPath = path;
	}

	function confirm() {
		onselect(selected);
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') oncancel();
		if (e.key === 'Enter') confirm();
	}

	/** Display a prettified path for the breadcrumb */
	const crumbs = $derived.by(() => {
		const parts = currentPath.replace(/\/+$/, '').split('/').filter(Boolean);
		const result: { label: string; path: string }[] = [{ label: '/', path: '/' }];
		let acc = '';
		for (const p of parts) {
			acc += '/' + p;
			result.push({ label: p, path: acc });
		}
		return result;
	});
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="picker-backdrop" role="presentation" onmousedown={oncancel} onkeydown={handleKeydown}>
	<div class="picker" role="dialog" aria-modal="true" aria-label="Open Folder" tabindex="0" onmousedown={(e) => e.stopPropagation()}>

		<div class="picker-header">
			<span class="picker-title">Open Folder</span>
			<button class="close-btn" onclick={oncancel} aria-label="Cancel">×</button>
		</div>

		<div class="breadcrumb" aria-label="Current path">
			{#each crumbs as crumb, i}
				{#if i > 0}<span class="crumb-sep">/</span>{/if}
				<button class="crumb" onclick={() => navigate(crumb.path)} title={crumb.path}>{crumb.label}</button>
			{/each}
		</div>

		<div class="dir-list" role="listbox" aria-label="Subdirectories">
			<button
				class="dir-item up-item"
				onclick={goUp}
				disabled={currentPath === '/'}
				aria-label="Go up to parent directory"
			>
				<span class="dir-icon">▲</span>
				<span class="dir-name">..</span>
			</button>

			{#if loading}
				<div class="loading">Loading…</div>
			{:else if error}
				<div class="dir-error">{error}</div>
			{:else if entries.length === 0}
				<div class="empty">No subdirectories</div>
			{:else}
				{#each entries as entry (entry.path)}
					<button
						class="dir-item"
						class:is-selected={entry.path === selected}
						role="option"
						aria-selected={entry.path === selected}
						onclick={() => { selected = entry.path; }}
						ondblclick={() => navigate(entry.path)}
						onkeydown={(e) => { if (e.key === 'Enter') navigate(entry.path); if (e.key === ' ') { e.preventDefault(); selected = entry.path; } }}
					>
						<span class="dir-icon">📁</span>
						<span class="dir-name">{entry.name}</span>
					</button>
				{/each}
			{/if}
		</div>

		<div class="current-selection" title={selected}>
			<span class="sel-label">Selected:</span>
			<span class="sel-path">{selected}</span>
		</div>

		<div class="picker-actions">
			<button class="picker-btn" onclick={oncancel}>Cancel</button>
			<button class="picker-btn primary" onclick={confirm}>Open "{selected.split('/').filter(Boolean).pop() || '/'}"</button>
		</div>
	</div>
</div>

<style>
	.picker-backdrop {
		position: fixed; inset: 0; z-index: 500;
		background: #00000099;
		display: flex; align-items: center; justify-content: center;
	}
	.picker {
		background: var(--surface); border: 1px solid var(--border); border-radius: 8px;
		width: clamp(360px, 50vw, 640px); max-height: 80vh;
		display: flex; flex-direction: column;
		box-shadow: 0 20px 60px #00000099;
		outline: none;
	}
	.picker-header {
		display: flex; align-items: center; justify-content: space-between;
		padding: 12px 16px; border-bottom: 1px solid var(--border); flex: 0 0 auto;
	}
	.picker-title { font-size: 14px; font-weight: 600; color: var(--text); }
	.close-btn { background: none; border: none; cursor: pointer; color: var(--muted); font-size: 18px; padding: 0 4px; border-radius: 4px; line-height: 1; }
	.close-btn:hover { color: var(--text); background: var(--hover-bg); }

	.breadcrumb {
		display: flex; align-items: center; flex-wrap: wrap; gap: 0;
		padding: 6px 12px; border-bottom: 1px solid var(--border);
		flex: 0 0 auto; overflow-x: auto; scrollbar-width: none;
		background: var(--sidebar-bg);
	}
	.breadcrumb::-webkit-scrollbar { display: none; }
	.crumb {
		background: none; border: none; cursor: pointer;
		color: var(--accent); font-size: 12px; padding: 2px 3px;
		border-radius: 3px; max-width: 160px; overflow: hidden;
		text-overflow: ellipsis; white-space: nowrap;
	}
	.crumb:hover { background: var(--hover-bg); }
	.crumb-sep { color: var(--muted); font-size: 11px; padding: 0 1px; user-select: none; }

	.dir-list {
		flex: 1 1 auto; overflow-y: auto; padding: 4px 8px;
		display: flex; flex-direction: column; gap: 1px; min-height: 0;
	}
	.dir-item {
		display: flex; align-items: center; gap: 8px;
		padding: 6px 10px; border-radius: 4px;
		background: none; border: none; cursor: pointer;
		color: var(--text); font-size: 13px; text-align: left; width: 100%;
		transition: background .1s;
	}
	.dir-item:hover:not(:disabled) { background: var(--hover-bg); }
	.dir-item.is-selected { background: var(--active-bg); color: var(--accent); }
	.dir-item:disabled { opacity: .35; cursor: default; }
	.up-item { color: var(--muted); }
	.up-item:hover:not(:disabled) { color: var(--text); }
	.dir-icon { flex: 0 0 auto; font-size: 14px; line-height: 1; }
	.dir-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.loading, .empty { padding: 16px; color: var(--muted); font-size: 12px; text-align: center; }
	.dir-error { padding: 8px 10px; color: var(--error); font-size: 12px; word-break: break-all; }

	.current-selection {
		display: flex; align-items: center; gap: 8px;
		padding: 8px 16px; border-top: 1px solid var(--border);
		flex: 0 0 auto; background: var(--sidebar-bg);
		font-size: 12px; overflow: hidden;
	}
	.sel-label { color: var(--muted); flex: 0 0 auto; }
	.sel-path { color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: 'JetBrains Mono', monospace; }

	.picker-actions {
		display: flex; justify-content: flex-end; gap: 8px;
		padding: 12px 16px; border-top: 1px solid var(--border); flex: 0 0 auto;
	}
	.picker-btn {
		padding: 6px 16px; background: var(--hover-bg); border: 1px solid var(--border);
		border-radius: var(--radius); color: var(--text); font-size: 13px; cursor: pointer;
	}
	.picker-btn:hover { background: var(--active-bg); }
	.picker-btn.primary { background: var(--accent); color: var(--bg); border-color: var(--accent); font-weight: 600; }
	.picker-btn.primary:hover { filter: brightness(1.1); }
</style>
