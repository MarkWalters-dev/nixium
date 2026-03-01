<script lang="ts">
	import type { AppSettings } from '$lib/types';
	import { EDITOR_OPTION_ITEMS } from '$lib/types';

	let {
		draft = $bindable(),
		ollamaModels,
		ollamaModelsLoading,
		ollamaModelsError,
		onsave,
		oncancel,
		onfetchollama,
	}: {
		draft: AppSettings;
		ollamaModels: string[];
		ollamaModelsLoading: boolean;
		ollamaModelsError: string;
		onsave: (d: AppSettings) => void;
		oncancel: () => void;
		onfetchollama: () => void;
	} = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="modal-backdrop" role="presentation" onmousedown={oncancel}>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div class="modal" role="dialog" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
		<div class="modal-title">⚙ Settings</div>
		<div class="settings-section">
			<div class="settings-heading">AI</div>
			<label class="modal-label">Provider
				<select bind:value={draft.ai.provider} class="modal-input"
					onchange={() => { if (draft.ai.provider === 'ollama') { onfetchollama(); } }}>
					<option value="openai">OpenAI</option>
					<option value="anthropic">Anthropic</option>
					<option value="ollama">Ollama (local)</option>
					<option value="custom">Custom (OpenAI-compatible)</option>
				</select>
			</label>
			<label class="modal-label">API Key
				<input type="password" bind:value={draft.ai.apiKey} class="modal-input modal-mono"
					placeholder={draft.ai.provider === 'ollama' ? 'Not required' : 'sk-…'} />
			</label>
			<label class="modal-label">Model
				{#if draft.ai.provider === 'ollama'}
					<div class="ollama-model-row">
						{#if ollamaModels.length > 0}
							<select bind:value={draft.ai.model} class="modal-input modal-mono">
								{#each ollamaModels as m}
									<option value={m}>{m}</option>
								{/each}
								{#if draft.ai.model && !ollamaModels.includes(draft.ai.model)}
									<option value={draft.ai.model}>{draft.ai.model}</option>
								{/if}
							</select>
						{:else}
							<input type="text" bind:value={draft.ai.model} class="modal-input modal-mono" placeholder="llama3.2" />
						{/if}
						<button class="modal-btn fetch-btn" onclick={onfetchollama} disabled={ollamaModelsLoading} title="Fetch models from Ollama">
							{ollamaModelsLoading ? '…' : '↻'}
						</button>
					</div>
					{#if ollamaModelsError}<span class="fetch-error">{ollamaModelsError}</span>{/if}
				{:else}
					<input type="text" bind:value={draft.ai.model} class="modal-input modal-mono"
						placeholder={draft.ai.provider === 'anthropic' ? 'claude-3-5-sonnet-20241022' : 'gpt-4o-mini'} />
				{/if}
			</label>
			{#if draft.ai.provider === 'ollama' || draft.ai.provider === 'custom'}
			<label class="modal-label">Base URL
				<input type="text" bind:value={draft.ai.baseUrl} class="modal-input modal-mono"
					placeholder={draft.ai.provider === 'ollama' ? 'http://localhost:11434' : 'https://api.example.com'} />
			</label>
			{/if}
		</div>
		<div class="settings-section">
			<div class="settings-heading">Editor</div>
			{#each EDITOR_OPTION_ITEMS as item}
				<label class="modal-label modal-label-row">
					<span class="modal-label-name">{item.label}</span>
					<span class="modal-label-desc">{item.desc}</span>
					<input
						type="checkbox"
						checked={draft.nixiumOptions[item.key]}
						onchange={(e) => { draft.nixiumOptions = { ...draft.nixiumOptions, [item.key]: (e.target as HTMLInputElement).checked }; }}
					/>
				</label>
			{/each}
		</div>
		<div class="modal-actions">
			<button class="modal-btn" onclick={oncancel}>Cancel</button>
			<button class="modal-btn primary" onclick={() => onsave(draft)}>Save</button>
		</div>
	</div>
</div>

<style>
	.modal-backdrop { position: fixed; inset: 0; z-index: 500; background: #00000088; display: flex; align-items: center; justify-content: center; }
	.modal { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 20px 24px; min-width: 380px; max-width: 500px; display: flex; flex-direction: column; gap: 14px; box-shadow: 0 20px 60px #00000099; }
	.modal-title { font-size: 14px; font-weight: 600; color: var(--text); }
	.settings-section { display: flex; flex-direction: column; gap: 10px; }
	.settings-heading { font-size: 10px; font-weight: 700; letter-spacing: .08em; color: var(--muted); text-transform: uppercase; padding-bottom: 4px; border-bottom: 1px solid var(--border); }
	.modal-label { display: flex; flex-direction: column; gap: 5px; font-size: 12px; color: var(--muted); }
	.modal-input { padding: 7px 10px; background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 13px; outline: none; width: 100%; }
	.modal-input:focus { border-color: var(--accent); }
	.modal-mono { font-family: 'JetBrains Mono', monospace; }
	.modal-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
	.modal-btn { padding: 6px 16px; background: var(--hover-bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 13px; cursor: pointer; }
	.modal-btn:hover { background: var(--active-bg); }
	.modal-btn.primary { background: var(--accent); color: var(--bg); border-color: var(--accent); font-weight: 600; }
	.modal-btn.primary:hover { filter: brightness(1.1); }
	.ollama-model-row { display: flex; gap: 6px; align-items: stretch; }
	.ollama-model-row .modal-input { flex: 1 1 auto; }
	.fetch-btn { flex: 0 0 auto; padding: 6px 10px; font-size: 14px; }
	.fetch-error { font-size: 11px; color: var(--error); margin-top: 2px; }
	.modal-label-row { flex-direction: row !important; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: wrap; }
	.modal-label-name { font-size: 13px; color: var(--text); min-width: 120px; }
	.modal-label-desc { font-size: 10px; color: var(--muted); flex: 1; }
</style>
