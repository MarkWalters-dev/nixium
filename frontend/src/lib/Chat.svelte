<script lang="ts">
	import { tick } from 'svelte';

	export interface ChatMessage {
		role: 'user' | 'assistant' | 'tool';
		content: string;
		error?: boolean;
		/** tool_calls present on assistant messages when the AI is calling a tool */
		tool_calls?: Array<{ id: string; type: string; function: { name: string; arguments: string } }>;
		/** id of the tool call this result is for */
		tool_call_id?: string;
		/** display name of the tool that was called */
		tool_name?: string;
	}
	export interface ChatThread { id: string; title: string; messages: ChatMessage[]; createdAt: number; }

	let {
		messages = [],
		threads = [],
		activeChatId = '',
		loading = false,
		activeFile = null,
		useContext = false,
		mode = 'panel',
		interactionMode = 'ask',
		model = '',
		ollamaModels = [],
		onsend,
		onnewchat,
		onswitchchat,
		onclose,
		onmovetotab,
		onmovetopanel,
		ontogglecontext,
		onchangemode,
		onchangemodel,
	}: {
		messages: ChatMessage[];
		threads: ChatThread[];
		activeChatId: string;
		loading: boolean;
		activeFile?: string | null;
		useContext?: boolean;
		mode?: 'panel' | 'tab';
		interactionMode?: 'ask' | 'plan' | 'agent';
		model?: string;
		ollamaModels?: string[];
		onsend: (text: string) => void;
		onnewchat: () => void;
		onswitchchat: (id: string) => void;
		onclose: () => void;
		onmovetotab: () => void;
		onmovetopanel: () => void;
		ontogglecontext: () => void;
		onchangemode: (m: 'ask' | 'plan' | 'agent') => void;
		onchangemodel: (m: string) => void;
	} = $props();

	let input = $state('');
	let scrollEl = $state<HTMLElement>();
	let historyOpen = $state(false);
	let modelDropOpen = $state(false);
	let modelInput = $state('');

	// Scroll to bottom whenever messages change or loading state changes.
	$effect(() => {
		void messages;
		void loading;
		tick().then(() => {
			if (scrollEl) scrollEl.scrollTop = scrollEl.scrollHeight;
		});
	});

	function send() {
		const text = input.trim();
		if (!text || loading) return;
		input = '';
		onsend(text);
	}

	function formatDate(ts: number): string {
		const d = new Date(ts);
		const now = new Date();
		if (d.toDateString() === now.toDateString())
			return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
		return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
	}

	// ── Simple content renderer ──────────────────────────────────────────────
	interface Segment { type: 'text' | 'code'; content: string; lang?: string; }

	function parseContent(text: string): Segment[] {
		const parts: Segment[] = [];
		const re = /```(\w*)\n?([\s\S]*?)```/g;
		let last = 0;
		let m: RegExpExecArray | null;
		while ((m = re.exec(text)) !== null) {
			if (m.index > last) parts.push({ type: 'text', content: text.slice(last, m.index) });
			parts.push({ type: 'code', content: m[2].trimEnd(), lang: m[1] || undefined });
			last = m.index + m[0].length;
		}
		if (last < text.length) parts.push({ type: 'text', content: text.slice(last) });
		return parts.length ? parts : [{ type: 'text', content: text }];
	}

	// Inline: bold (**text**) and inline code (`code`)
	function inlineHtml(text: string): string {
		return text
			.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
			.replace(/\*\*(.*?)\*\*/g, '<strong>$1</strong>')
			.replace(/`([^`]+)`/g, '<code class="inline-code">$1</code>');
	}

	async function copyCode(content: string) {
		try { await navigator.clipboard.writeText(content); } catch { /* ignore */ }
	}
</script>

<div class="chat">
	<div class="chat-bar">
		<span class="chat-title">✦ AI</span>
		<div class="chat-bar-actions">
			{#if activeFile && !historyOpen}
				<button class="icon-btn chat-act" class:ctx-on={useContext}
					title={useContext ? 'File context: ON (click to disable)' : 'Add current file as context'}
					onclick={ontogglecontext}>
					📎
				</button>
			{/if}
			<button class="icon-btn chat-act" class:history-active={historyOpen}
				title="Chat history" onclick={() => (historyOpen = !historyOpen)}>🕐</button>
			<button class="icon-btn chat-act" title="New chat" onclick={() => { onnewchat(); historyOpen = false; }}>＋</button>
			<button class="icon-btn chat-act"
				title={mode === 'tab' ? 'Move to panel' : 'Move to tab'}
				onclick={mode === 'tab' ? onmovetopanel : onmovetotab}>
				{mode === 'tab' ? '⊟' : '⊞'}
			</button>
			<button class="icon-btn chat-act" onclick={onclose} title="Close AI Chat">×</button>
		</div>
	</div>

	{#if historyOpen}
		<div class="history-panel">
			{#if threads.length === 0 || (threads.length === 1 && threads[0].messages.length === 0)}
				<div class="history-empty">No previous chats</div>
			{:else}
				{#each threads as t (t.id)}
					<button
						class="history-item"
						class:active={t.id === activeChatId}
						onclick={() => { onswitchchat(t.id); historyOpen = false; }}
					>
						<span class="history-item-title">{t.title}</span>
						<span class="history-item-time">{formatDate(t.createdAt)}</span>
					</button>
				{/each}
			{/if}
		</div>
	{:else}
		<div class="chat-messages" bind:this={scrollEl} tabindex="0">
		{#if messages.length === 0}
			<div class="chat-empty">
				<div class="chat-empty-icon">✦</div>
				<div>Ask anything • get code suggestions • explain errors</div>
				{#if activeFile}
					<div class="chat-hint-small">Toggle 📎 to include the open file as context</div>
				{/if}
			</div>
		{/if}

		{#each messages as msg (msg)}
			{#if msg.role === 'tool'}
				<!-- Tool result card -->
				<div class="msg msg-tool-result">
					<div class="tool-result-card">
						<span class="tool-result-icon">🔧</span>
						<span class="tool-result-name">{msg.tool_name ?? 'tool'}</span>
						<span class="tool-result-status" class:tool-ok={!msg.error} class:tool-err={msg.error}>
							{msg.error ? '✗' : '✓'}
						</span>
						{#if msg.content && msg.content.length < 200}
							<span class="tool-result-text">{msg.content}</span>
						{:else if msg.content}
							<span class="tool-result-text">{msg.content.slice(0, 160)}…</span>
						{/if}
					</div>
				</div>
			{:else}
				<div class="msg" class:msg-user={msg.role === 'user'} class:msg-error={msg.error}>
					<div class="msg-role">{msg.role === 'user' ? 'You' : 'AI'}</div>
					<div class="msg-body">
						{#if msg.tool_calls?.length && !msg.content}
							<!-- Assistant called tools -->
							{#each msg.tool_calls as tc}
								<div class="tool-call-badge">
									<span class="tool-call-icon">⚙</span>
									<span>Calling <code class="inline-code">{tc.function.name}</code></span>
								</div>
							{/each}
						{:else}
							{#each parseContent(msg.content) as seg}
								{#if seg.type === 'code'}
									<div class="code-block">
										<div class="code-header">
											<span class="code-lang">{seg.lang ?? 'code'}</span>
											<button class="icon-btn code-copy" onclick={() => copyCode(seg.content)} title="Copy">⎘</button>
										</div>
										<pre class="code-pre"><code>{seg.content}</code></pre>
									</div>
								{:else}
									<!-- eslint-disable-next-line svelte/no-at-html-tags -->
									<p class="msg-text">{@html inlineHtml(seg.content)}</p>
								{/if}
							{/each}
						{/if}
					</div>
				</div>
			{/if}
		{/each}

		{#if loading}
			<div class="msg">
				<div class="msg-role">AI</div>
				<div class="msg-body"><span class="typing-cursor">▋</span></div>
			</div>
		{/if}
		</div>

		<div class="chat-input-area">
			<!-- Mode + model toolbar -->
			<div class="chat-toolbar">
				<div class="mode-seg">
					<button class="mode-btn" class:active={interactionMode === 'ask'} onclick={() => onchangemode('ask')}>Ask</button>
					<button class="mode-btn" class:active={interactionMode === 'plan'} onclick={() => onchangemode('plan')}>Plan</button>
					<button class="mode-btn" class:active={interactionMode === 'agent'} onclick={() => onchangemode('agent')}>Agent</button>
				</div>
				<div class="model-picker">
					{#if ollamaModels.length > 0}
						<select class="model-select" value={model} onchange={(e) => onchangemodel((e.target as HTMLSelectElement).value)} title="Model">
							{#each ollamaModels as m}<option value={m}>{m}</option>{/each}
							{#if model && !ollamaModels.includes(model)}<option value={model}>{model}</option>{/if}
						</select>
					{:else}
						<button class="model-text-btn" onclick={() => { modelDropOpen = !modelDropOpen; modelInput = model; }} title="Change model">
							<span class="model-name">{model || 'model'}</span><span class="model-chevron">▾</span>
						</button>
						{#if modelDropOpen}
							<div class="model-drop">
								<input class="model-drop-input" type="text" bind:value={modelInput} placeholder="model name"
									onkeydown={(e) => { if (e.key === 'Enter') { onchangemodel(modelInput); modelDropOpen = false; } if (e.key === 'Escape') modelDropOpen = false; }} />
								<button class="model-drop-ok" onclick={() => { onchangemodel(modelInput); modelDropOpen = false; }}>✓</button>
							</div>
						{/if}
					{/if}
				</div>
			</div>
			{#if useContext && activeFile}
				<div class="ctx-badge">📎 {activeFile.split('/').pop()}</div>
			{/if}
			<div class="chat-input-row">
				<!-- svelte-ignore a11y_autofocus -->
				<textarea
					class="chat-input"
					bind:value={input}
					placeholder="Ask a question… (Enter to send, Shift+Enter for newline)"
					rows="2"
					autofocus
					onkeydown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } }}
				></textarea>
				<button class="send-btn" onclick={send} disabled={loading || !input.trim()} title="Send (Enter)">↑</button>
			</div>
		</div>
	{/if}
</div>

<style>
	.chat { display: flex; flex-direction: column; height: 100%; overflow: hidden; background: var(--bg); }

	.chat-bar { display: flex; align-items: center; justify-content: space-between; padding: 4px 8px; background: var(--sidebar-bg); border-bottom: 1px solid var(--border); flex: 0 0 auto; min-height: 32px; }
	.chat-title { font-size: 11px; font-weight: 700; letter-spacing: .06em; color: var(--accent); user-select: none; }
	.chat-bar-actions { display: flex; align-items: center; gap: 2px; }
	.chat-act { font-size: 13px; padding: 2px 5px; }
	.chat-act.ctx-on { color: var(--accent) !important; }
	.chat-act.history-active { color: var(--accent) !important; }

	.history-panel { flex: 1 1 auto; overflow-y: auto; display: flex; flex-direction: column; gap: 1px; padding: 4px; scrollbar-width: thin; scrollbar-color: var(--border) transparent; }
	.history-empty { color: var(--muted); font-size: 12px; text-align: center; padding: 24px 0; }
	.history-item { display: flex; flex-direction: column; gap: 2px; padding: 8px 10px; border-radius: var(--radius); background: none; border: none; cursor: pointer; text-align: left; color: var(--text); transition: background .1s; }
	.history-item:hover { background: var(--hover-bg); }
	.history-item.active { background: var(--active-bg); outline: 1px solid var(--border); }
	.history-item-title { font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
	.history-item-time { font-size: 10px; color: var(--muted); }

	.chat-messages { flex: 1 1 auto; overflow-y: auto; padding: 8px 0; display: flex; flex-direction: column; gap: 2px; scrollbar-width: thin; scrollbar-color: var(--border) transparent; }

	.chat-empty { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; color: var(--muted); font-size: 12px; text-align: center; padding: 16px; }
	.chat-empty-icon { font-size: 28px; color: var(--border); }
	.chat-hint-small { font-size: 11px; color: var(--border); margin-top: 4px; }

	.msg { display: flex; flex-direction: column; gap: 4px; padding: 8px 12px; border-bottom: 1px solid var(--border); }
	.msg-user { background: var(--surface); }
	.msg-error .msg-body { color: var(--error); }
	.msg-role { font-size: 10px; font-weight: 700; letter-spacing: .07em; color: var(--muted); text-transform: uppercase; }
	.msg-user .msg-role { color: var(--accent); }
	.msg-body { font-size: 13px; line-height: 1.55; color: var(--text); }
	.msg-text { margin: 0; white-space: pre-wrap; word-break: break-word; }
	.msg-text + .msg-text { margin-top: 6px; }
	:global(.inline-code) { font-family: 'JetBrains Mono', monospace; font-size: 12px; background: var(--hover-bg); border: 1px solid var(--border); border-radius: 3px; padding: 0 4px; color: var(--info); }

	/* Tool call badges inside assistant messages */
	.tool-call-badge { display: inline-flex; align-items: center; gap: 5px; font-size: 11px; color: var(--muted); background: var(--hover-bg); border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 8px; margin: 2px 0; }
	.tool-call-icon { font-size: 12px; }

	/* Tool result row between messages */
	.msg-tool-result { padding: 3px 12px; border-bottom: none; }
	.tool-result-card { display: inline-flex; align-items: center; gap: 6px; font-size: 11px; color: var(--muted); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); padding: 3px 10px; max-width: 100%; }
	.tool-result-icon { font-size: 11px; flex-shrink: 0; }
	.tool-result-name { font-family: monospace; font-weight: 600; color: var(--text); flex-shrink: 0; }
	.tool-result-status { font-weight: 700; flex-shrink: 0; }
	.tool-ok { color: #a6e3a1; }
	.tool-err { color: var(--error); }
	.tool-result-text { color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 260px; }

	.code-block { margin: 6px 0; border: 1px solid var(--border); border-radius: 5px; overflow: hidden; }
	.code-header { display: flex; align-items: center; justify-content: space-between; padding: 2px 8px; background: var(--sidebar-bg); border-bottom: 1px solid var(--border); }
	.code-lang { font-size: 10px; font-weight: 600; color: var(--muted); letter-spacing: .05em; text-transform: uppercase; }
	.code-copy { font-size: 12px; padding: 1px 5px; }
	.code-pre { margin: 0; padding: 10px 12px; overflow-x: auto; background: #11111b; font-size: 12.5px; line-height: 1.5; }
	.code-pre code { font-family: 'JetBrains Mono', 'Fira Code', monospace; color: var(--text); }

	.typing-cursor { animation: blink .9s step-end infinite; }
	@keyframes blink { 0%, 100% { opacity: 1; } 50% { opacity: 0; } }

	.chat-input-area { flex: 0 0 auto; border-top: 1px solid var(--border); padding: 6px; display: flex; flex-direction: column; gap: 4px; }
	.chat-toolbar { display: flex; align-items: center; justify-content: space-between; gap: 6px; }
	.mode-seg { display: flex; background: var(--hover-bg); border-radius: var(--radius); padding: 2px; gap: 1px; }
	.mode-btn { background: none; border: none; padding: 3px 9px; border-radius: calc(var(--radius) - 1px); font-size: 11px; font-weight: 600; color: var(--muted); cursor: pointer; transition: background .1s, color .1s; }
	.mode-btn.active { background: var(--surface); color: var(--accent); }
	.mode-btn:hover:not(.active) { color: var(--text); }
	.model-picker { position: relative; display: flex; align-items: center; }
	.model-select { background: var(--hover-bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--muted); font-size: 11px; padding: 3px 6px; outline: none; max-width: 120px; }
	.model-text-btn { display: flex; align-items: center; gap: 3px; background: var(--hover-bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--muted); font-size: 11px; padding: 3px 7px; cursor: pointer; }
	.model-text-btn:hover { color: var(--text); }
	.model-name { max-width: 100px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.model-chevron { font-size: 9px; }
	.model-drop { position: absolute; bottom: calc(100% + 4px); right: 0; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); padding: 4px; display: flex; gap: 4px; box-shadow: 0 4px 16px #00000066; z-index: 20; }
	.model-drop-input { background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 12px; padding: 4px 7px; outline: none; width: 160px; font-family: monospace; }
	.model-drop-input:focus { border-color: var(--accent); }
	.model-drop-ok { background: var(--accent); border: none; border-radius: var(--radius); color: var(--bg); font-size: 13px; padding: 4px 8px; cursor: pointer; font-weight: 700; }
	.ctx-badge { font-size: 10px; color: var(--accent); padding: 1px 6px; background: var(--surface); border-radius: 10px; align-self: flex-start; }
	.chat-input-row { display: flex; gap: 6px; align-items: flex-end; }
	.chat-input { flex: 1; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 13px; padding: 7px 10px; resize: none; outline: none; font-family: inherit; line-height: 1.45; }
	.chat-input:focus { border-color: var(--accent); }
	.send-btn { flex: 0 0 auto; width: 32px; height: 32px; background: var(--accent); border: none; border-radius: var(--radius); color: var(--bg); font-size: 16px; cursor: pointer; display: flex; align-items: center; justify-content: center; font-weight: 700; transition: background .12s; }
	.send-btn:hover:not(:disabled) { background: #a8c7fa; }
	.send-btn:disabled { opacity: .4; cursor: default; }
</style>
