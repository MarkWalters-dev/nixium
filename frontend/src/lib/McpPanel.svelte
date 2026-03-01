<script lang="ts">
	import type { McpToolInfo, ExternalMcpServer, ExternalMcpToolInfo } from '$lib/types';

	let {
		tools,
		loading,
		detailName,
		externalServers = [],
		externalToolsByServer = {},
		onclose,
		onopendetail,
		ontoggle,
		onaddserver,
		ondeleteserver,
		ontoggleserver,
		onfetchservertools,
		ontoggleservertool,
	}: {
		tools: McpToolInfo[];
		loading: boolean;
		detailName: string | null;
		externalServers?: ExternalMcpServer[];
		externalToolsByServer?: Record<string, ExternalMcpToolInfo[]>;
		onclose: () => void;
		onopendetail: (name: string) => void;
		ontoggle: (name: string) => void;
		onaddserver?: (cfg: Omit<ExternalMcpServer, 'id' | 'enabled'>) => Promise<void>;
		ondeleteserver?: (id: string) => Promise<void>;
		ontoggleserver?: (id: string) => Promise<void>;
		onfetchservertools?: (id: string) => Promise<void>;
		ontoggleservertool?: (serverId: string, toolName: string) => Promise<void>;
	} = $props();

	// ── add-server form ───────────────────────────────────────────────────────
	let addOpen       = $state(false);
	let addName       = $state('');
	let addCommand    = $state('');
	let addArgs       = $state('');
	let addEnv        = $state('');
	let addError      = $state('');
	let addBusy       = $state(false);

	// which servers have their tool list expanded
	let expanded = $state<Set<string>>(new Set());

	function toggleExpand(id: string) {
		const next = new Set(expanded);
		if (next.has(id)) {
			next.delete(id);
		} else {
			next.add(id);
			if (!externalToolsByServer[id]) onfetchservertools?.(id);
		}
		expanded = next;
	}

	async function submitAdd() {
		addError = '';
		if (!addName.trim() || !addCommand.trim()) { addError = 'Name and command are required.'; return; }
		let env: Record<string, string> = {};
		if (addEnv.trim()) {
			try { env = JSON.parse(addEnv.trim()); }
			catch {
				// try KEY=VALUE lines
				try {
					for (const line of addEnv.trim().split('\n')) {
						const eq = line.indexOf('=');
						if (eq > 0) env[line.slice(0, eq).trim()] = line.slice(eq + 1).trim();
					}
				} catch { addError = 'Invalid env format. Use JSON or KEY=VALUE lines.'; return; }
			}
		}
		addBusy = true;
		try {
			await onaddserver?.({
				name: addName.trim(),
				command: addCommand.trim(),
				args: addArgs.trim() ? addArgs.trim().split(/\s+/) : [],
				env,
			});
			addName = ''; addCommand = ''; addArgs = ''; addEnv = '';
			addOpen = false;
		} catch (e: unknown) {
			addError = e instanceof Error ? e.message : 'Failed to add server.';
		} finally { addBusy = false; }
	}
</script>

<div class="mcp-panel">
	<div class="mcp-header">
		<span class="mcp-title">MCP Skills</span>
		<button class="icon-btn mcp-close" onclick={onclose} title="Close">×</button>
	</div>
	<div class="mcp-desc">
		Toggle which AI tools are available in agent mode. Enabled tools are passed to the AI and can be called during agent tasks.
	</div>

	<!-- ── built-in tools ── -->
	<div class="section-header">Built-in</div>
	{#if loading}
		<div class="mcp-loading">Loading…</div>
	{:else if tools.length === 0}
		<div class="mcp-empty">No built-in skills found.</div>
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

	<!-- ── external servers ── -->
	<div class="section-header ext-section-header">
		<span>External Servers</span>
		<button class="icon-btn" onclick={() => { addOpen = !addOpen; addError = ''; }} title={addOpen ? 'Cancel' : 'Add server'}>
			{addOpen ? '×' : '+'}
		</button>
	</div>

	{#if addOpen}
		<form class="add-form" onsubmit={(e) => { e.preventDefault(); submitAdd(); }}>
			<label class="add-label">Name
				<input class="add-input" bind:value={addName} placeholder="My MCP Server" disabled={addBusy} />
			</label>
			<label class="add-label">Command
				<input class="add-input" bind:value={addCommand} placeholder="npx -y @foo/mcp-server" disabled={addBusy} />
			</label>
			<label class="add-label">Args <span class="add-hint">(space-separated, optional)</span>
				<input class="add-input" bind:value={addArgs} placeholder="--port 3000" disabled={addBusy} />
			</label>
			<label class="add-label">Env <span class="add-hint">(JSON or KEY=VALUE lines, optional)</span>
				<textarea class="add-input add-textarea" bind:value={addEnv} placeholder={'API_KEY=abc\nDEBUG=1'} rows={3} disabled={addBusy}></textarea>
			</label>
			{#if addError}<div class="add-error">{addError}</div>{/if}
			<button class="add-submit" type="submit" disabled={addBusy}>{addBusy ? 'Adding…' : 'Add Server'}</button>
		</form>
	{/if}

	{#if externalServers.length === 0 && !addOpen}
		<div class="mcp-empty">No external servers configured.</div>
	{/if}

	{#each externalServers as srv}
		<div class="srv-card" class:srv-enabled={srv.enabled}>
			<div class="srv-row">
				<button class="mcp-item-btn srv-name-btn" onclick={() => toggleExpand(srv.id)}>
					<span class="srv-expand">{expanded.has(srv.id) ? '▾' : '▸'}</span>
					<span class="mcp-item-label">{srv.name}</span>
					<span class="mcp-item-desc srv-cmd">{srv.command}{srv.args.length ? ' ' + srv.args.join(' ') : ''}</span>
				</button>
				<div class="srv-actions">
					<input
						type="checkbox"
						checked={srv.enabled}
						onchange={() => ontoggleserver?.(srv.id)}
						class="mcp-toggle"
						title={srv.enabled ? 'Disable server' : 'Enable server'}
					/>
					<button class="icon-btn srv-delete" onclick={() => ondeleteserver?.(srv.id)} title="Remove server">🗑</button>
				</div>
			</div>

			{#if expanded.has(srv.id)}
				<div class="srv-tools">
					{#if !externalToolsByServer[srv.id]}
						<div class="mcp-loading srv-tools-loading">
							Loading tools…
							<button class="icon-btn" onclick={() => onfetchservertools?.(srv.id)}>↺</button>
						</div>
					{:else if externalToolsByServer[srv.id].length === 0}
						<div class="mcp-empty">No tools found.
							<button class="icon-btn" onclick={() => onfetchservertools?.(srv.id)} title="Refresh">↺</button>
						</div>
					{:else}
						<div class="srv-tools-bar">
							<span class="srv-tools-count">{externalToolsByServer[srv.id].length} tool{externalToolsByServer[srv.id].length !== 1 ? 's' : ''}</span>
							<button class="icon-btn" onclick={() => onfetchservertools?.(srv.id)} title="Refresh tools">↺</button>
						</div>
						{#each externalToolsByServer[srv.id] as t}
							<div class="mcp-item srv-tool-item" class:mcp-item-enabled={t.enabled}>
								<div class="mcp-item-btn" style="pointer-events:none">
									<span class="mcp-item-label">{t.name}</span>
									<span class="mcp-item-desc">{t.description}</span>
								</div>
								<input
									type="checkbox"
									checked={t.enabled}
									onchange={() => ontoggleservertool?.(srv.id, t.name)}
									class="mcp-toggle"
									title={t.enabled ? 'Disable tool' : 'Enable tool'}
								/>
							</div>
						{/each}
					{/if}
				</div>
			{/if}
		</div>
	{/each}
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

	/* section headers */
	.section-header { font-size: 9px; font-weight: 700; text-transform: uppercase; letter-spacing: .08em; color: var(--muted); padding: 6px 10px 3px; background: var(--sidebar-bg, var(--bg)); border-bottom: 1px solid var(--border); flex: 0 0 auto; }
	.ext-section-header { display: flex; align-items: center; justify-content: space-between; }
	.ext-section-header .icon-btn { font-size: 16px; }

	/* add-server form */
	.add-form { display: flex; flex-direction: column; gap: 6px; padding: 8px 10px; border-bottom: 1px solid var(--border); background: var(--hover-bg); }
	.add-label { display: flex; flex-direction: column; gap: 2px; font-size: 10px; color: var(--muted); }
	.add-hint { font-size: 9px; color: var(--muted); opacity: .7; }
	.add-input { font-size: 11px; background: var(--input-bg, var(--bg)); color: var(--text); border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 5px; outline: none; font-family: inherit; width: 100%; box-sizing: border-box; }
	.add-input:focus { border-color: var(--accent); }
	.add-textarea { resize: vertical; min-height: 52px; }
	.add-error { font-size: 10px; color: #e06c75; }
	.add-submit { align-self: flex-start; font-size: 11px; background: var(--accent); color: #fff; border: none; border-radius: var(--radius); padding: 4px 10px; cursor: pointer; }
	.add-submit:disabled { opacity: .6; cursor: default; }

	/* server cards */
	.srv-card { border-bottom: 1px solid var(--border); }
	.srv-enabled { border-left: 2px solid var(--accent); }
	.srv-row { display: flex; align-items: flex-start; gap: 6px; padding: 7px 10px; }
	.srv-name-btn { flex-direction: row; gap: 4px; align-items: baseline; flex-wrap: wrap; }
	.srv-expand { font-size: 9px; opacity: .7; flex: 0 0 auto; }
	.srv-cmd { word-break: break-all; }
	.srv-actions { display: flex; align-items: center; gap: 4px; flex: 0 0 auto; }
	.srv-delete { font-size: 12px; }
	.srv-tools { border-top: 1px solid var(--border); background: var(--hover-bg); }
	.srv-tools-bar { display: flex; align-items: center; justify-content: space-between; padding: 4px 10px; }
	.srv-tools-count { font-size: 9px; color: var(--muted); }
	.srv-tool-item { padding-left: 20px; }
	.srv-tools-loading { display: flex; align-items: center; gap: 6px; }
</style>
