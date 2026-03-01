<script lang="ts">
	export interface SearchMatch { path: string; line: number; col: number; text: string; }

	let {
		rootPath,
		onjump,
		onclose,
	}: {
		rootPath: string;
		onjump: (match: SearchMatch) => void;
		onclose: () => void;
	} = $props();

	let query         = $state('');
	let caseSensitive = $state(false);
	let loading       = $state(false);
	let results       = $state<SearchMatch[]>([]);
	let error         = $state('');

	const fileGroups = $derived(
		[...new Map(
			results.reduce((m, r) => {
				if (!m.has(r.path)) m.set(r.path, []);
				m.get(r.path)!.push(r);
				return m;
			}, new Map<string, SearchMatch[]>())
		).entries()]
	);

	async function runSearch() {
		if (!query.trim()) { results = []; return; }
		loading = true;
		error = '';
		try {
			const url = `/api/fs/search?path=${encodeURIComponent(rootPath)}&query=${encodeURIComponent(query)}&caseSensitive=${caseSensitive}`;
			const res = await fetch(url);
			if (!res.ok) {
				const body = await res.json().catch(() => ({ error: res.statusText }));
				throw new Error((body as { error?: string }).error ?? res.statusText);
			}
			results = await res.json() as SearchMatch[];
		} catch (err) {
			error = (err as Error).message;
			results = [];
		} finally {
			loading = false;
		}
	}
</script>

<div class="fif-panel">
	<div class="fif-header">
		<span class="fif-title">Find in Files</span>
		<button class="icon-btn fif-close" onclick={onclose} title="Close">×</button>
	</div>
	<div class="fif-inputs">
		<input
			class="fif-input"
			placeholder="Search…"
			bind:value={query}
			onkeydown={(e) => { if (e.key === 'Enter') runSearch(); }}
		/>
		<div class="fif-options">
			<label class="fif-opt-label">
				<input type="checkbox" bind:checked={caseSensitive} />
				Aa
			</label>
			<button class="fif-go-btn" onclick={runSearch} disabled={loading}>
				{loading ? '…' : 'Find'}
			</button>
		</div>
	</div>
	{#if error}
		<div class="fif-msg fif-err">{error}</div>
	{/if}
	{#if fileGroups.length > 0}
		<div class="fif-summary">
			{results.length} match{results.length !== 1 ? 'es' : ''} in {fileGroups.length} file{fileGroups.length !== 1 ? 's' : ''}
		</div>
		<div class="fif-results">
			{#each fileGroups as [filePath, fileMatches]}
				<div class="fif-file-group">
					<div class="fif-fname" title={filePath}>
						<span class="fif-fname-base">{filePath.split('/').pop()}</span>
						<span class="fif-fname-dir">{filePath.split('/').slice(0,-1).join('/')}</span>
					</div>
					{#each fileMatches as match}
						<button class="fif-match" onclick={() => onjump(match)} title="{filePath}:{match.line}">
							<span class="fif-lnum">{match.line}</span>
							<span class="fif-ltext">{match.text.trimStart()}</span>
						</button>
					{/each}
				</div>
			{/each}
		</div>
	{:else if !loading && query.trim() && !error}
		<div class="fif-msg">No results.</div>
	{/if}
</div>

<style>
	.fif-panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
	.fif-header { display: flex; align-items: center; justify-content: space-between; padding: 6px 8px 4px; flex: 0 0 auto; border-bottom: 1px solid var(--border); }
	.fif-title { font-size: 10px; font-weight: 700; letter-spacing: .07em; text-transform: uppercase; color: var(--muted); }
	.fif-close { font-size: 15px; padding: 1px 4px; }
	.fif-inputs { display: flex; flex-direction: column; gap: 5px; padding: 6px 8px; flex: 0 0 auto; border-bottom: 1px solid var(--border); }
	.fif-input { width: 100%; background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 12px; padding: 5px 8px; outline: none; font-family: 'JetBrains Mono', monospace; }
	.fif-input:focus { border-color: var(--accent); }
	.fif-options { display: flex; align-items: center; gap: 6px; }
	.fif-opt-label { display: flex; align-items: center; gap: 4px; font-size: 11px; color: var(--muted); cursor: pointer; user-select: none; padding: 2px 6px; border: 1px solid transparent; border-radius: var(--radius); }
	.fif-opt-label:hover { border-color: var(--border); }
	.fif-opt-label input { accent-color: var(--accent); }
	.fif-go-btn { margin-left: auto; padding: 3px 10px; background: var(--accent); border: none; border-radius: var(--radius); color: var(--bg); font-size: 11px; font-weight: 700; cursor: pointer; }
	.fif-go-btn:hover:not(:disabled) { filter: brightness(1.1); }
	.fif-go-btn:disabled { opacity: .5; cursor: default; }
	.fif-summary { font-size: 10px; color: var(--muted); padding: 4px 10px; flex: 0 0 auto; }
	.fif-msg { font-size: 11px; color: var(--muted); padding: 8px 10px; }
	.fif-err { color: var(--error); }
	.fif-results { flex: 1 1 auto; overflow-y: auto; overflow-x: hidden; }
	.fif-file-group { border-bottom: 1px solid var(--border); }
	.fif-fname { display: flex; flex-direction: column; padding: 5px 8px 2px; background: var(--sidebar-bg); position: sticky; top: 0; z-index: 1; }
	.fif-fname-base { font-size: 11px; font-weight: 600; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.fif-fname-dir { font-size: 9px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: 'JetBrains Mono', monospace; }
	.fif-match { display: flex; align-items: baseline; gap: 6px; width: 100%; background: none; border: none; padding: 2px 8px; cursor: pointer; text-align: left; min-width: 0; }
	.fif-match:hover { background: var(--hover-bg); }
	.fif-lnum { flex: 0 0 auto; font-size: 10px; color: var(--muted); font-family: 'JetBrains Mono', monospace; min-width: 24px; text-align: right; }
	.fif-ltext { flex: 1 1 auto; font-size: 11px; color: var(--text); font-family: 'JetBrains Mono', monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.icon-btn { flex: 0 0 auto; background: none; border: none; cursor: pointer; color: var(--muted); font-size: 16px; padding: 4px 6px; border-radius: var(--radius); line-height: 1; transition: color .12s, background .12s; }
	.icon-btn:hover { color: var(--text); background: var(--hover-bg); }
</style>
