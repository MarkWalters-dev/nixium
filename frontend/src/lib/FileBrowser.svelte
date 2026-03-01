<script lang="ts">
	import { untrack } from 'svelte';
	interface TreeNode {
		name: string;
		path: string;
		isDir: boolean;
		depth: number;
		expanded: boolean;
		loaded: boolean;
	}
	interface FsEntry { name: string; path: string; is_dir: boolean; }

	let {
		rootPath = '/',
		activeFile = null as string | null,
		onopen,
	}: {
		rootPath?: string;
		activeFile?: string | null;
		onopen?: (path: string) => void;
	} = $props();

	let nodes = $state<TreeNode[]>([]);
	let loadingPaths = $state(new Set<string>());
	let errorMsg = $state('');

	const folderName = $derived(rootPath.split('/').filter(Boolean).pop() || rootPath);

	$effect(() => {
		const path = rootPath;
		untrack(() => loadRoot(path));
	});

	async function fetchChildren(path: string): Promise<FsEntry[]> {
		const res = await fetch(`/api/fs/list?path=${encodeURIComponent(path)}`);
		if (!res.ok) {
			const body = await res.json().catch(() => ({ error: res.statusText }));
			throw new Error(body.error ?? res.statusText);
		}
		return res.json();
	}

	async function loadRoot(path: string) {
		errorMsg = '';
		loadingPaths = new Set([...loadingPaths, path]);
		try {
			const entries = await fetchChildren(path);
			nodes = entries.map((e) => ({
name: e.name, path: e.path, isDir: e.is_dir,
depth: 0, expanded: false, loaded: false,
}));
		} catch (err) {
			errorMsg = (err as Error).message;
			nodes = [];
		} finally {
			const next = new Set(loadingPaths);
			next.delete(path);
			loadingPaths = next;
		}
	}

	function dirIcon(expanded: boolean) { return expanded ? '▾' : '▸'; }

	async function toggle(index: number) {
		const node = nodes[index];
		if (!node.isDir) return;
		if (node.expanded) {
			let end = index + 1;
			while (end < nodes.length && nodes[end].depth > node.depth) end++;
			nodes.splice(index + 1, end - (index + 1));
			nodes[index].expanded = false;
			return;
		}
		if (!node.loaded) {
			loadingPaths = new Set([...loadingPaths, node.path]);
			try {
				const entries = await fetchChildren(node.path);
				const children: TreeNode[] = entries.map((e) => ({
name: e.name, path: e.path, isDir: e.is_dir,
depth: node.depth + 1, expanded: false, loaded: false,
}));
				nodes[index].loaded = true;
				nodes.splice(index + 1, 0, ...children);
			} catch (err) {
				errorMsg = (err as Error).message;
			} finally {
				const next = new Set(loadingPaths);
				next.delete(node.path);
				loadingPaths = next;
			}
		}
		nodes[index].expanded = true;
	}

	function fileClass(name: string): string {
		const ext = name.split('.').pop()?.toLowerCase() ?? '';
		if (['rs'].includes(ext)) return 'lang-rs';
		if (['js','mjs','cjs','ts','tsx','jsx'].includes(ext)) return 'lang-js';
		if (['svelte'].includes(ext)) return 'lang-svelte';
		if (['py'].includes(ext)) return 'lang-py';
		if (['nix'].includes(ext)) return 'lang-nix';
		if (['json','jsonc'].includes(ext)) return 'lang-json';
		if (['html','htm'].includes(ext)) return 'lang-html';
		if (['css','scss','sass'].includes(ext)) return 'lang-css';
		if (['md','mdx'].includes(ext)) return 'lang-md';
		if (['sh','bash','zsh'].includes(ext)) return 'lang-sh';
		if (['toml','yaml','yml'].includes(ext)) return 'lang-toml';
		return 'lang-default';
	}
</script>

<div class="browser">
	<div class="explorer-header">
		<span class="explorer-title">{folderName.toUpperCase()}</span>
		<button class="header-btn" onclick={() => loadRoot(rootPath)} title="Refresh">↻</button>
	</div>

	{#if errorMsg}
		<div class="error-strip">{errorMsg}</div>
	{/if}

	<div class="tree" role="tree" aria-label="File browser">
		{#each nodes as node, i (node.path)}
			<div
				class="node"
				class:is-dir={node.isDir}
				class:is-active={!node.isDir && node.path === activeFile}
				style="--depth: {node.depth}"
				role="treeitem"
				aria-selected={node.path === activeFile}
				aria-expanded={node.isDir ? node.expanded : undefined}
				tabindex="0"
				onclick={() => node.isDir ? toggle(i) : onopen?.(node.path)}
				onkeydown={(e) => {
					if (e.key === 'Enter' || e.key === ' ') {
						e.preventDefault();
						if (node.isDir) toggle(i); else onopen?.(node.path);
					}
				}}
			>
				{#if node.isDir}
					<span class="icon dir-icon">
						{#if loadingPaths.has(node.path)}<span class="spin">⟳</span>
						{:else}{dirIcon(node.expanded)}{/if}
					</span>
					<span class="icon folder-icon">{node.expanded ? '📂' : '📁'}</span>
				{:else}
					<span class="icon file-dot {fileClass(node.name)}" aria-hidden="true"></span>
				{/if}
				<span class="label" title={node.path}>{node.name}</span>
			</div>
		{/each}
		{#if nodes.length === 0 && !errorMsg}
			<div class="empty">Empty directory</div>
		{/if}
	</div>
</div>

<style>
	.browser { display:flex; flex-direction:column; height:100%; overflow:hidden; background:var(--sidebar-bg); font-size:13px; }
	.explorer-header { display:flex; align-items:center; justify-content:space-between; padding:5px 8px 5px 12px; border-bottom:1px solid var(--border); flex:0 0 auto; min-height:28px; }
	.explorer-title { font-size:11px; font-weight:700; letter-spacing:.06em; color:var(--muted); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
	.header-btn { background:none; border:none; cursor:pointer; color:var(--muted); font-size:14px; padding:2px 4px; border-radius:3px; line-height:1; flex:0 0 auto; }
	.header-btn:hover { color:var(--text); background:var(--hover-bg); }
	.error-strip { padding:4px 10px; background:#3b1212; color:var(--error); font-size:11px; flex:0 0 auto; word-break:break-all; }
	.tree { flex:1 1 auto; overflow-y:auto; overflow-x:hidden; padding:4px 0; }
	.node { display:flex; align-items:center; gap:3px; padding:2px 8px 2px calc(8px + var(--depth) * 14px); cursor:pointer; white-space:nowrap; border-radius:3px; margin:0 4px; color:var(--text); user-select:none; min-height:24px; }
	.node:hover { background:var(--hover-bg); }
	.node:focus { outline:1px solid var(--accent); outline-offset:-1px; }
	.node.is-active { background:var(--active-bg); color:var(--accent); }
	.icon { flex:0 0 auto; line-height:1; }
	.dir-icon { font-size:10px; color:var(--muted); width:12px; text-align:center; }
	.folder-icon { font-size:14px; }
	.file-dot { width:8px; height:8px; border-radius:50%; margin:0 2px; }
	.lang-rs     { background:#f46b4f; }
	.lang-js     { background:#f0db4f; }
	.lang-svelte { background:#ff3e00; }
	.lang-py     { background:#4b8bbe; }
	.lang-nix    { background:#7ebae4; }
	.lang-json   { background:#a0c878; }
	.lang-html   { background:#e44d26; }
	.lang-css    { background:#264de4; }
	.lang-md     { background:#cdd6f4; }
	.lang-sh     { background:#a6e3a1; }
	.lang-toml   { background:#cba6f7; }
	.lang-default { background:var(--muted); }
	.label { overflow:hidden; text-overflow:ellipsis; font-family:system-ui,sans-serif; font-size:13px; }
	.is-dir > .label { color:var(--text); }
	.empty { padding:12px; color:var(--muted); font-size:12px; text-align:center; }
	@keyframes rotate { to { transform:rotate(360deg); } }
	.spin { display:inline-block; animation:rotate .8s linear infinite; }
</style>
