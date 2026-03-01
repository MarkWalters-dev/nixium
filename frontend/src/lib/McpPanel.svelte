<script lang="ts">
	export interface McpToolInfo {
		name: string;
		displayName: string;
		description: string;
		enabled: boolean;
		inputSchema: Record<string, unknown>;
	}

	let {
		tools,
		loading,
		detailName,
		onclose,
		onopendetail,
		ontoggle,
	}: {
		tools: McpToolInfo[];
		loading: boolean;
		detailName: string | null;
		onclose: () => void;
		onopendetail: (name: string) => void;
		ontoggle: (name: string) => void;
	} = $props();
</script>

<div class="mcp-panel">
	<div class="mcp-header">
		<span class="mcp-title">MCP Skills</span>
		<button class="icon-btn mcp-close" onclick={onclose} title="Close">×</button>
	</div>
	<div class="mcp-desc">
		Toggle which AI tools are available in agent mode. Enabled tools are passed to the AI and can be called during agent tasks.
	</div>
	{#if loading}
		<div class="mcp-loading">Loading…</div>
	{:else if tools.length === 0}
		<div class="mcp-empty">No MCP skills found.</div>
	{:else}
		{#each tools as tool}
			<div class="mcp-item" class:mcp-item-enabled={tool.enabled} class:mcp-item-active={detailName === tool.name}>
				<button class="mcp-item-info mcp-item-btn" onclick={() => onopendetail(tool.name)}>
					<span class="mcp-item-label">{tool.displayName}</span>
					<span class="mcp-item-desc">{tool.description}</span>
				</button>
				<input
					type="checkbox"
					checked={tool.enabled}
					onchange={() => ontoggle(tool.name)}
					class="mcp-toggle"
					title={tool.enabled ? 'Disable this skill' : 'Enable this skill'}
				/>
			</div>
		{/each}
	{/if}
</div>

<style>
	.mcp-panel { display: flex; flex-direction: column; height: 100%; overflow-y: auto; }
	.mcp-header { display: flex; align-items: center; justify-content: space-between; padding: 6px 8px 4px; flex: 0 0 auto; border-bottom: 1px solid var(--border); }
	.mcp-title { font-size: 10px; font-weight: 700; letter-spacing: .07em; text-transform: uppercase; color: var(--muted); }
	.mcp-close, .icon-btn { flex: 0 0 auto; background: none; border: none; cursor: pointer; color: var(--muted); font-size: 15px; padding: 1px 4px; border-radius: var(--radius); line-height: 1; transition: color .12s, background .12s; }
	.icon-btn:hover { color: var(--text); background: var(--hover-bg); }
	.mcp-desc { font-size: 10px; color: var(--muted); padding: 6px 10px; line-height: 1.5; border-bottom: 1px solid var(--border); flex: 0 0 auto; }
	.mcp-loading, .mcp-empty { font-size: 11px; color: var(--muted); padding: 10px; }
	.mcp-item { display: flex; align-items: flex-start; justify-content: space-between; gap: 8px; padding: 8px 10px; border-bottom: 1px solid var(--border); transition: background .1s; }
	.mcp-item:hover { background: var(--hover-bg); }
	.mcp-item-enabled { border-left: 2px solid var(--accent); padding-left: 8px; }
	.mcp-item-btn { background: none; border: none; cursor: pointer; text-align: left; padding: 0; color: inherit; flex: 1 1 auto; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
	.mcp-item-btn:hover .mcp-item-label { color: var(--accent); }
	.mcp-item-active { background: var(--hover-bg); }
	.mcp-item-label { font-size: 12px; color: var(--text); font-weight: 500; }
	.mcp-item-desc { font-size: 10px; color: var(--muted); line-height: 1.4; white-space: normal; }
	.mcp-toggle { flex: 0 0 auto; width: 16px; height: 16px; accent-color: var(--accent); cursor: pointer; margin-top: 2px; }
</style>
