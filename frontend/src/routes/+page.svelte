<script lang="ts">
	import { tick } from 'svelte';
	import { createCodeMirrorAction } from '$lib/useCodeMirror';
	import FileBrowser from '$lib/FileBrowser.svelte';
	import FolderPicker from '$lib/FolderPicker.svelte';
	import Terminal from '$lib/Terminal.svelte';
	import Chat, { type ChatMessage, type ChatThread } from '$lib/Chat.svelte';
	import CommandPalette from '$lib/CommandPalette.svelte';
	import SettingsModal from '$lib/SettingsModal.svelte';
	import FindInFilesPanel, { type SearchMatch } from '$lib/FindInFilesPanel.svelte';
	import ExtensionsPanel, { type StoreEntry } from '$lib/ExtensionsPanel.svelte';
	import McpPanel from '$lib/McpPanel.svelte';
	import { marked } from 'marked';

	import { type EditorExtensionKey } from '$lib/useCodeMirror';
	import type { ExtensionManifest } from '$lib/extensions';
	import { type AppSettings, type PaletteCommand, DEFAULT_SETTINGS, loadSettings, SETTINGS_KEY } from '$lib/types';

	interface Tab { path: string; name: string; content: string; dirty: boolean; }
	type StatusKind = 'idle' | 'info' | 'success' | 'error';

	const ROOT_KEY     = 'nixium-root';
	const RECENT_KEY   = 'nixium-recent-folders';
	const AUTOSAVE_KEY = 'nixium-autosave';
	const TERM_TAB     = '__terminal__';
	const CHAT_TAB     = '__chat__';
	const MAX_RECENT   = 8;

	function loadRecent(): string[] {
		try { return JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]'); } catch { return []; }
	}
	function saveRecent(list: string[]) { localStorage.setItem(RECENT_KEY, JSON.stringify(list)); }

	// ── State ─────────────────────────────────────────────────────────────────
	let rootPath        = $state(typeof localStorage !== 'undefined' ? (localStorage.getItem(ROOT_KEY) ?? '/') : '/');
	let sidebarVisible  = $state(true);
	let sidebarWidth    = $state(220);
	let isDragging      = $state(false);
	let terminalVisible = $state(false);
	let terminalMode    = $state<'panel' | 'tab'>('panel');
	let terminalHeight  = $state(220);
	let isTermDragging  = $state(false);
	let terminalRef     = $state<{ focus: () => void; sendText: (t: string) => void }>();
	let menuOpen        = $state(false);
	let openFolderMode  = $state(false);
	let recentFolders   = $state<string[]>(typeof localStorage !== 'undefined' ? loadRecent() : []);
	let tabs            = $state<Tab[]>([]);
	let activeTabPath   = $state<string | null>(null);
	let tabStatus       = $state<Record<string, { msg: string; kind: StatusKind }>>({});
	let autosave        = $state(typeof localStorage !== 'undefined' ? localStorage.getItem(AUTOSAVE_KEY) === '1' : false);
	let statusBarVisible = $state(typeof localStorage !== 'undefined' ? localStorage.getItem('nixium-statusbar') !== '0' : true);
	let runMsg          = $state<{ msg: string; kind: StatusKind } | null>(null);
	function _genChatId() { return Date.now().toString(36) + Math.random().toString(36).slice(2, 6); }
	const _initChatId = _genChatId();
	let chatThreads     = $state<ChatThread[]>([{ id: _initChatId, title: 'New Chat', messages: [], createdAt: Date.now() }]);
	let activeChatId    = $state(_initChatId);
	let chatLoading     = $state(false);
	let chatUseContext  = $state(false);
	let chatVisible     = $state(false);
	let chatMode        = $state<'panel' | 'tab'>('panel');
	let chatWidth       = $state(340);
	let isChatDragging  = $state(false);
	let chatInteractionMode = $state<'ask' | 'plan' | 'agent'>('ask');
	let settings        = $state<AppSettings>(typeof localStorage !== 'undefined' ? loadSettings() : DEFAULT_SETTINGS);
	let settingsOpen    = $state(false);
	let settingsDraft   = $state<AppSettings>(DEFAULT_SETTINGS);
	let ollamaModels    = $state<string[]>([]);
	let ollamaModelsLoading = $state(false);
	let ollamaModelsError   = $state('');
	let saveAsOpen      = $state(false);
	let saveAsDraft     = $state('');
	let newFileOpen     = $state(false);
	let newFileDraft    = $state('');

	// ── Extensions panel ─────────────────────────────────────────────────────
	let extOpen      = $state(false);
	let extList      = $state<ExtensionManifest[]>([]);
	let extCommands  = $state<PaletteCommand[]>([]);
	const extHandles = new Map<string, () => void | Promise<void>>();
	let notification = $state<{ msg: string; type: 'info' | 'error'; id: number } | null>(null);
	let installingExt = $state<string | null>(null);
	let removingExt   = $state<string | null>(null);

	// ── Extension store ───────────────────────────────────────────────────────
	let extDetailName          = $state<string | null>(null);
	let extDetailReadmeHtml    = $state<string>('');
	let extDetailReadmeLoading = $state(false);
	let extDetailStoreEntry    = $state<StoreEntry | null>(null);

	// ── MCP Skills panel ─────────────────────────────────────────────────────
	interface McpToolInfo {
		name: string;
		displayName: string;
		description: string;
		enabled: boolean;
		inputSchema: Record<string, unknown>;
	}
	let mcpOpen         = $state(false);
	let mcpTools        = $state<McpToolInfo[]>([]);
	let mcpToolsLoading = $state(false);
	let mcpDetailName          = $state<string | null>(null);
	let mcpDetailReadmeHtml    = $state<string>('');
	let mcpDetailReadmeLoading = $state(false);

	async function fetchMcpTools() {
		mcpToolsLoading = true;
		try {
			const res = await fetch('/api/mcp/tools');
			if (res.ok) mcpTools = await res.json();
		} catch { /* ignore */ } finally { mcpToolsLoading = false; }
	}

	async function toggleMcpTool(name: string) {
		try {
			const res = await fetch(`/api/mcp/tools/${encodeURIComponent(name)}/toggle`, { method: 'POST' });
			if (res.ok) {
				const updated: McpToolInfo = await res.json();
				mcpTools = mcpTools.map(t => t.name === name ? updated : t);
			}
		} catch { /* ignore */ }
	}

	async function openMcpDetail(name: string) {
		mcpDetailName = name;
		mcpDetailReadmeHtml = '';
		mcpDetailReadmeLoading = true;
		try {
			const res = await fetch(`/api/mcp/tools/${encodeURIComponent(name)}/readme`);
			if (res.ok) {
				const text = await res.text();
				mcpDetailReadmeHtml = await Promise.resolve(marked.parse(text)) as string;
			}
		} catch { /* ignore */ }
		finally { mcpDetailReadmeLoading = false; }
	}

	function closeMcpDetail() {
		mcpDetailName = null;
		mcpDetailReadmeHtml = '';
	}

	function openMcp() {
		mcpOpen    = true;
		fifOpen    = false;
		extOpen    = false;
		sidebarVisible = true;
		menuOpen   = false;
		if (mcpTools.length === 0) fetchMcpTools();
	}

	// ── Find in Files ─────────────────────────────────────────────────────────
	let fifOpen = $state(false);

	// ── Command Palette ───────────────────────────────────────────────────────
	let paletteOpen = $state(false);

	const activeTab  = $derived(tabs.find((t) => t.path === activeTabPath) ?? null);
	const isTermTab  = $derived(terminalMode === 'tab' && activeTabPath === TERM_TAB);
	const isChatTab  = $derived(chatMode === 'tab' && chatVisible && activeTabPath === CHAT_TAB);
	const hideEditor = $derived(isTermTab || isChatTab || !!extDetailName || !!mcpDetailName);
	const lineCount  = $derived(activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB ? activeTab.content.split('\n').length : 0);

	const paletteCommands = $derived([
		// Always available
		{ id: 'newfile',        label: 'File: New File…',                      action: openNewFileModal },
		{ id: 'openfolder',     label: 'File: Open Folder…',                   action: openFolderPrompt },
		{ id: 'togglesidebar',  label: 'View: Toggle Explorer',                keybinding: 'Ctrl+B',       action: () => { sidebarVisible = !sidebarVisible; } },
		{ id: 'toggleterm',     label: 'View: Toggle Terminal',                keybinding: 'Ctrl+`',       action: toggleTerminal },
		{ id: 'termlayout',     label: `View: Terminal → ${terminalMode === 'tab' ? 'Panel' : 'Tab'}`, action: () => { if (!terminalVisible) setTerminalVisible(true); setTerminalMode(terminalMode === 'tab' ? 'panel' : 'tab'); } },
		{ id: 'togglestatusbar',label: `View: ${statusBarVisible ? 'Hide' : 'Show'} Status Bar`,  keybinding: 'Ctrl+J',       action: () => { statusBarVisible = !statusBarVisible; } },
		{ id: 'toggleautosave', label: `File: ${autosave ? 'Disable' : 'Enable'} Autosave`,        action: () => { autosave = !autosave; } },
		{ id: 'togglechat',     label: 'AI: Toggle Chat',                      keybinding: 'Ctrl+K',       action: toggleChat },
		{ id: 'newchat',        label: 'AI: New Chat Thread',                  action: newChat },
		{ id: 'chatlayout',     label: `AI: Chat → ${chatMode === 'tab' ? 'Panel' : 'Tab'}`,   action: () => { if (!chatVisible) setChatVisible(true); setChatMode(chatMode === 'tab' ? 'panel' : 'tab'); } },
		{ id: 'findinfiles',    label: 'Search: Find in Files',                keybinding: 'Ctrl+Shift+F', action: openFindInFiles },
		{ id: 'extensions',     label: 'View: Extensions',                     keybinding: 'Ctrl+Shift+X', action: openExtensions },
		{ id: 'mcpskills',      label: 'View: MCP Skills',                     keybinding: 'Ctrl+Shift+M', action: openMcp },
		{ id: 'runproject',     label: 'Run: Run Project',                     keybinding: 'Ctrl+Enter',   action: runProject },
		{ id: 'settings',       label: 'Preferences: Open Settings',           action: openSettings },
		// Requires an open file
		...(activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB ? [
			{ id: 'save',     label: 'File: Save',                             keybinding: 'Ctrl+S', action: saveActiveTab },
			{ id: 'saveas',   label: 'File: Save As…',                         action: openSaveAs },
			{ id: 'closetab', label: 'View: Close Tab',                         keybinding: 'Ctrl+W', action: () => { if (activeTabPath) closeTab(activeTabPath); } },
			{ id: 'search',   label: 'Search: Find / Replace in Editor',         keybinding: 'Ctrl+F', action: () => nixium.openSearch() },
		] as PaletteCommand[] : []),
		// Commands registered by loaded extensions
		...extCommands,
	] as PaletteCommand[]);

	// Persist preferences
	$effect(() => { localStorage.setItem(AUTOSAVE_KEY, autosave ? '1' : '0'); });
	$effect(() => { localStorage.setItem('nixium-statusbar', statusBarVisible ? '1' : '0'); });
	$effect(() => { localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings)); });
	// Apply editor option toggles to the live editor whenever settings change
	$effect(() => {
		const ext = settings.nixiumOptions;
		for (const k of Object.keys(ext) as EditorExtensionKey[]) {
			nixium.setEditorExtension(k, ext[k]);
		}
	});
	// Load/unload real extensions when their enabled state changes
	$effect(() => {
		const enabled = settings.extensions;
		for (const ext of extList) {
			if (enabled[ext.name]) { loadExtension(ext.name); }
			else if (extHandles.has(ext.name)) { unloadExtension(ext.name); }
		}
	});
	// Filtering is handled by ExtensionsPanel internally.
	// Auto-fetch Ollama models when chat becomes visible with Ollama provider
	$effect(() => {
		if (chatVisible && settings.ai.provider === 'ollama' && ollamaModels.length === 0) {
			fetchOllamaModels(settings.ai.baseUrl || undefined, false);
		}
	});
	// Autosave debounce
	let _autosaveTimer: ReturnType<typeof setTimeout> | null = null;
	$effect(() => {
		if (!autosave) return;
		const dirtyPaths = tabs.filter((t) => t.dirty).map((t) => t.path);
		if (dirtyPaths.length === 0) return;
		if (_autosaveTimer) clearTimeout(_autosaveTimer);
		_autosaveTimer = setTimeout(() => {
			dirtyPaths.forEach((p) => { const t = tabs.find((x) => x.path === p); if (t?.dirty) _saveTab(t.path, t.content); });
		}, 1500);
		return () => { if (_autosaveTimer) clearTimeout(_autosaveTimer); };
	});

	function status(path: string) { return tabStatus[path] ?? { msg: '', kind: 'idle' as StatusKind }; }
	function setStatus(path: string, msg: string, kind: StatusKind) {
		tabStatus = { ...tabStatus, [path]: { msg, kind } };
	}

	/** Svelte action: fires callback when the user clicks outside the node. */
	function clickOutside(node: HTMLElement, callback: () => void) {
		function handler(e: MouseEvent) {
			if (!node.contains(e.target as Node)) callback();
		}
		document.addEventListener('mousedown', handler, true);
		return { destroy() { document.removeEventListener('mousedown', handler, true); } };
	}

	// ── CodeMirror ────────────────────────────────────────────────────────────
	const nixium = createCodeMirrorAction({
		initialValue: '',
		filename: '',
		onChange: (value: string) => {
			if (!activeTabPath || activeTabPath === TERM_TAB || activeTabPath === CHAT_TAB) return;
			const tab = tabs.find((t) => t.path === activeTabPath);
			if (tab && value !== tab.content) {
				tab.dirty = true;
				tab.content = value;
			}
		},
	});

	/** Apply all persisted editor option settings to the live editor view. */
	function applyAllExtensions() {
		const ext = settings.nixiumOptions;
		for (const k of Object.keys(ext) as EditorExtensionKey[]) {
			nixium.setEditorExtension(k, ext[k]);
		}
	}

	async function fetchExtensions() {
		try {
			const res = await fetch('/api/extensions');
			if (res.ok) {
				extList = await res.json();
			}
		} catch { /* ignore */ }
	}

	function showNotif(msg: string, type: 'info' | 'error' = 'info') {
		const id = Date.now();
		notification = { msg, type, id };
		setTimeout(() => { if (notification?.id === id) notification = null; }, 3500);
	}

	async function openExtDetail(name: string) {
		extDetailName = name;
		const installed = extList.find(e => e.name === name);
		extDetailStoreEntry = extDetailStoreEntry?.name === name
			? extDetailStoreEntry
			: installed
				? { name: installed.name, displayName: installed.displayName, version: installed.version, description: installed.description, download_url: '' }
				: null;
		extDetailReadmeHtml = '';
		extDetailReadmeLoading = true;
		try {
			const res = await fetch(`/api/extensions/${encodeURIComponent(name)}/readme`);
			if (res.ok) {
				const text = await res.text();
				extDetailReadmeHtml = await Promise.resolve(marked.parse(text)) as string;
			}
		} catch { /* ignore */ }
		finally { extDetailReadmeLoading = false; }
	}

	async function openStoreExtDetail(entry: StoreEntry) {
		extDetailName = entry.name;
		extDetailStoreEntry = entry;
		extDetailReadmeHtml = '';
		extDetailReadmeLoading = true;
		try {
			if (entry.readme_url) {
				const res = await fetch(entry.readme_url);
				if (res.ok) {
					const text = await res.text();
					extDetailReadmeHtml = await Promise.resolve(marked.parse(text)) as string;
				}
			} else if (entry.description) {
				extDetailReadmeHtml = `<p>${entry.description}</p>`;
			}
		} catch { /* ignore */ }
		finally { extDetailReadmeLoading = false; }
	}

	function closeExtDetail() {
		extDetailName = null;
		extDetailReadmeHtml = '';
		extDetailStoreEntry = null;
	}

	async function installExtension(entry: StoreEntry) {
		installingExt = entry.name;
		try {
			const res = await fetch('/api/extensions/store/install', {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ name: entry.name, download_url: entry.download_url }),
			});
			if (res.ok) {
				await fetchExtensions();
				settings.extensions = { ...settings.extensions, [entry.name]: true };
				showNotif(`"${entry.displayName}" installed`);
			} else {
				const body = await res.json().catch(() => ({ error: res.statusText }));
				showNotif((body as { error?: string }).error ?? 'Install failed', 'error');
			}
		} catch (e) { showNotif((e as Error).message, 'error'); }
		finally { installingExt = null; }
	}

	async function removeExtension(name: string) {
		removingExt = name;
		try {
			const res = await fetch(`/api/extensions/${encodeURIComponent(name)}`, { method: 'DELETE' });
			if (res.ok) {
				const next = { ...settings.extensions }; delete next[name];
				settings.extensions = next;
				await fetchExtensions();
				showNotif('Extension removed');
			} else { showNotif('Remove failed', 'error'); }
		} catch (e) { showNotif((e as Error).message, 'error'); }
		finally { removingExt = null; }
	}

	function buildExtensionAPI(): import('$lib/extensions').ExtensionAPI {
		return {
			getActiveFilePath: () => activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB ? activeTabPath : null,
			getActiveFileContent: () => activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB ? activeTab.content : null,
			openFile: async (path: string) => { await openFile(path); },
			registerCommand(id, label, handler) {
				extCommands = [...extCommands, { id, label, action: handler }];
				return () => { extCommands = extCommands.filter(c => c.id !== id); };
			},
			showNotification(msg, type = 'info') {
				const id = Date.now();
				notification = { msg, type, id };
				setTimeout(() => { if (notification?.id === id) notification = null; }, 3500);
			},
		};
	}

	async function loadExtension(name: string) {
		if (extHandles.has(name)) return;
		try {
			const mod = await import(/* @vite-ignore */ `/api/extensions/${encodeURIComponent(name)}/script`);
			const registeredIds: string[] = [];
			const api: import('$lib/extensions').ExtensionAPI = {
				...buildExtensionAPI(),
				registerCommand(id, label, handler) {
					const fullId = `ext:${name}:${id}`;
					registeredIds.push(fullId);
					extCommands = [...extCommands, { id: fullId, label, action: handler }];
					return () => { extCommands = extCommands.filter(c => c.id !== fullId); };
				},
			};
			await mod.activate?.(api);
			extHandles.set(name, async () => {
				try { await mod.deactivate?.(); } catch { /* ignore */ }
				extCommands = extCommands.filter(c => !registeredIds.includes(c.id));
			});
		} catch (err) {
			const id = Date.now();
			notification = { msg: `Failed to load extension "${name}": ${(err as Error).message}`, type: 'error', id };
			setTimeout(() => { if (notification?.id === id) notification = null; }, 4000);
		}
	}

	async function unloadExtension(name: string) {
		const deactivate = extHandles.get(name);
		if (!deactivate) return;
		extHandles.delete(name);
		try { await deactivate(); } catch { /* ignore */ }
	}

	async function activateTab(path: string) {
		if (path === TERM_TAB) {
			activeTabPath = TERM_TAB;
			setTimeout(() => terminalRef?.focus(), 50);
			return;
		}
		if (path === CHAT_TAB) {
			activeTabPath = CHAT_TAB;
			return;
		}
		const tab = tabs.find((t) => t.path === path);
		if (!tab) return;
		activeTabPath = path;
		await tick();
		nixium.setValue(tab.content);
		nixium.setLanguage(tab.name);
		nixium.requestMeasure();
		applyAllExtensions();
		nixium.focus();
	}

	// ── File operations ───────────────────────────────────────────────────────
	async function openFile(path: string) {
		const existing = tabs.find((t) => t.path === path);
		if (existing) { activateTab(path); sidebarVisible = false; return; }
		setStatus(path, 'Loading…', 'info');
		try {
			const res = await fetch(`/api/fs/read?path=${encodeURIComponent(path)}`);
			if (!res.ok) {
				const body = await res.json().catch(() => ({ error: res.statusText }));
				throw new Error(body.error ?? res.statusText);
			}
			const content = await res.text();
			const name = path.split('/').filter(Boolean).pop() ?? path;
			tabs = [...tabs, { path, name, content, dirty: false }];
			activeTabPath = path;
			sidebarVisible = false;
			await tick(); // ensure nixium-area is visible before setValue
			nixium.setValue(content);
			nixium.setLanguage(name);
			nixium.requestMeasure();
			applyAllExtensions();
			nixium.focus();
			setStatus(path, '', 'idle');
		} catch (err) {
			setStatus(path, (err as Error).message, 'error');
			tabs = tabs.filter((t) => t.path !== path);
			if (tabs.length > 0) activeTabPath = tabs[tabs.length - 1].path;
		}
	}

	async function _saveTab(path: string, content: string) {
		setStatus(path, 'Saving…', 'info');
		try {
			const res = await fetch('/api/fs/write', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ path, content }),
			});
			if (!res.ok) {
				const body = await res.json().catch(() => ({ error: res.statusText }));
				throw new Error(body.error ?? res.statusText);
			}
			const tab = tabs.find((t) => t.path === path);
			if (tab) { tab.dirty = false; }
			setStatus(path, 'Saved ✓', 'success');
		} catch (err) {
			setStatus(path, (err as Error).message, 'error');
		}
	}

	async function saveActiveTab() {
		if (!activeTab || activeTabPath === TERM_TAB || activeTabPath === CHAT_TAB) return;
		await _saveTab(activeTab.path, activeTab.content);
	}

	function openSaveAs() {
		if (!activeTab || activeTabPath === TERM_TAB || activeTabPath === CHAT_TAB) return;
		saveAsDraft = activeTab.path;
		saveAsOpen = true;
		menuOpen = false;
	}
	async function confirmSaveAs() {
		const newPath = saveAsDraft.trim();
		if (!newPath || !activeTab) return;
		await _saveTab(activeTab.path, activeTab.content);
		// update tab to new path
		const name = newPath.split('/').filter(Boolean).pop() ?? newPath;
		tabs = tabs.map(t => t.path === activeTab!.path ? { ...t, path: newPath, name } : t);
		activeTabPath = newPath;
		await _saveTab(newPath, activeTab.content);
		saveAsOpen = false;
	}

	function openNewFileModal() {
		newFileDraft = rootPath.replace(/\/$/, '') + '/';
		newFileOpen = true;
		menuOpen = false;
	}
	async function confirmNewFile() {
		const path = newFileDraft.trim();
		if (!path) return;
		newFileOpen = false;
		const name = path.split('/').filter(Boolean).pop() ?? path;
		tabs = [...tabs, { path, name, content: '', dirty: false }];
		activeTabPath = path;
		await tick();
		nixium.setValue('');
		nixium.setLanguage(name);
		nixium.focus();
		await _saveTab(path, '');
		newFileDraft = '';
	}

	// ── Find-in-files helpers ────────────────────────────────────────────────
	function openFindInFiles() {
		fifOpen = true;
		extOpen = false;
		mcpOpen = false;
		sidebarVisible = true;
	}

	function openExtensions() {
		extOpen = true;
		fifOpen = false;
		mcpOpen = false;
		sidebarVisible = true;
		fetchExtensions();
	}

	async function jumpToSearchResult(match: SearchMatch) {
		const wasFifOpen = fifOpen;
		const existing = tabs.find((t) => t.path === match.path);
		if (!existing) {
			await openFile(match.path);
		} else {
			await activateTab(match.path);
		}
		// openFile() hides the sidebar; restore FiF panel if it was open
		if (wasFifOpen) { fifOpen = true; sidebarVisible = true; }
		await tick();
		nixium.jumpToLine(match.line, match.col);
	}

	function closeTab(path: string) {
		if (path === TERM_TAB) { setTerminalVisible(false); return; }
		if (path === CHAT_TAB) { setChatVisible(false); return; }
		const idx = tabs.findIndex((t) => t.path === path);
		if (idx === -1) return;
		const wasActive = activeTabPath === path;
		tabs = tabs.filter((t) => t.path !== path);
		const { [path]: _, ...rest } = tabStatus;
		tabStatus = rest;
		if (wasActive) {
			const next = tabs[Math.min(idx, tabs.length - 1)];
			if (next) activateTab(next.path);
			else { activeTabPath = null; nixium.setValue(''); }
		}
	}

	// ── Chat ─────────────────────────────────────────────────────────────────────
	function setChatVisible(v: boolean) {
		chatVisible = v;
		if (v && chatMode === 'tab') {
			activeTabPath = CHAT_TAB;
		} else if (!v && activeTabPath === CHAT_TAB) {
			const last = tabs[tabs.length - 1];
			if (last) activateTab(last.path); else activeTabPath = null;
		}
	}
	function toggleChat() { setChatVisible(!chatVisible); }
	function setChatMode(mode: 'panel' | 'tab') {
		chatMode = mode;
		if (mode === 'tab' && chatVisible) { activeTabPath = CHAT_TAB; }
		else if (mode === 'panel' && activeTabPath === CHAT_TAB) {
			const last = tabs[tabs.length - 1];
			if (last) activateTab(last.path); else activeTabPath = null;
		}
	}

	function newChat() {
		const id = _genChatId();
		chatThreads = [{ id, title: 'New Chat', messages: [], createdAt: Date.now() }, ...chatThreads];
		activeChatId = id;
	}
	function switchChat(id: string) { activeChatId = id; }

	// ── Agent tool definitions (OpenAI native function-calling format) ─────────
	const AGENT_TOOLS = [
		{ type: 'function', function: {
			name: 'write_file',
			description: 'Write (create or overwrite) a file on disk.',
			parameters: { type: 'object', required: ['path', 'content'],
				properties: {
					path: { type: 'string', description: 'File path relative to the project root, or absolute.' },
					content: { type: 'string', description: 'Full text content to write.' },
				}
			}
		}},
		{ type: 'function', function: {
			name: 'read_file',
			description: 'Read the text content of a file.',
			parameters: { type: 'object', required: ['path'],
				properties: { path: { type: 'string' } }
			}
		}},
		{ type: 'function', function: {
			name: 'list_directory',
			description: 'List files and directories inside a directory.',
			parameters: { type: 'object', required: ['path'],
				properties: { path: { type: 'string' } }
			}
		}},
	];

	/** Resolve a path from the agent (possibly relative) to an absolute path. */
	function _resolveAgentPath(p: string): string {
		if (p.startsWith('/')) return p;
		return `${rootPath.replace(/\/$/, '')}/${p}`;
	}

	/** Build the messages array to send to the AI API. Strips internal tool messages for XML mode. */
	function _buildApiMessages(msgs: ChatMessage[], xmlMode: boolean): unknown[] {
		if (xmlMode) {
			// For XML mode: collapse tool result messages back into user turns
			const out: unknown[] = [];
			for (const m of msgs) {
				if (m.role === 'tool') {
					out.push({ role: 'user', content: `Tool result for ${m.tool_name ?? 'tool'}:\n${m.content}` });
				} else if (!m.tool_calls?.length) {
					out.push({ role: m.role as string, content: m.content });
				}
			}
			return out;
		}
		// Collect all tool_call IDs that have a proper assistant tool_calls message.
		// Any role:'tool' message whose id isn't in this set is from the XML fallback path
		// and must be collapsed to a user message — OpenAI rejects orphaned tool messages.
		const nativeCallIds = new Set<string>();
		for (const m of msgs) {
			if (m.tool_calls?.length) for (const tc of m.tool_calls) nativeCallIds.add(tc.id);
		}
		return msgs.map((m) => {
			if (m.role === 'tool') {
				if (m.tool_call_id && nativeCallIds.has(m.tool_call_id)) {
					return { role: 'tool', tool_call_id: m.tool_call_id, content: m.content };
				}
				// Orphaned tool result (XML fallback) → user message so providers don't reject it.
				return { role: 'user', content: `Tool result for ${m.tool_name ?? 'tool'}:\n${m.content}` };
			}
			if (m.tool_calls?.length) return { role: 'assistant', content: m.content ?? '', tool_calls: m.tool_calls };
			return { role: m.role as string, content: m.content };
		});
	}

	/** Execute a single agent action (write/read/list) and return { result, isErr }. */
	async function _execAgentAction(name: string, args: Record<string, string>): Promise<{ result: string; isErr: boolean }> {
		let result = '';
		let isErr = false;
		try {
			if (name === 'write_file') {
				const path = _resolveAgentPath(args.path ?? '');
				const r = await fetch('/api/fs/write', {
					method: 'POST',
					headers: { 'Content-Type': 'application/json' },
					body: JSON.stringify({ path, content: args.content ?? '' }),
				});
				if (r.ok) {
					result = `Wrote ${args.path}`;
					const existing = tabs.find((t) => t.path === path);
					if (existing) {
						existing.content = args.content ?? '';
						existing.dirty = false;
						if (activeTabPath === path) { await tick(); nixium.setValue(existing.content); }
					} else {
						openFile(path);
					}
				} else {
					const e = await r.json().catch(() => ({ error: r.statusText })) as { error?: string };
					result = `Error writing ${args.path}: ${e.error ?? r.statusText}`;
					isErr = true;
				}
			} else if (name === 'read_file') {
				const path = _resolveAgentPath(args.path ?? '');
				const r = await fetch(`/api/fs/read?path=${encodeURIComponent(path)}`);
				if (r.ok) { result = await r.text(); }
				else { result = `Error reading ${args.path}: ${r.statusText}`; isErr = true; }
			} else if (name === 'list_directory') {
				const path = _resolveAgentPath(args.path ?? '');
				const r = await fetch(`/api/fs/list?path=${encodeURIComponent(path)}`);
				if (r.ok) {
					const entries = await r.json() as Array<{ name: string; is_dir: boolean }>;
					result = entries.map((e) => `${e.is_dir ? 'd' : 'f'} ${e.name}`).join('\n') || '(empty)';
				} else { result = `Error listing ${args.path}: ${r.statusText}`; isErr = true; }
			} else {
				// Unknown built-in tool → try calling via the MCP endpoint
				try {
					const res = await fetch('/api/mcp/call', {
						method: 'POST',
						headers: { 'Content-Type': 'application/json' },
						body: JSON.stringify({ name, arguments: args }),
					});
					const data = await res.json() as { content: string; is_error: boolean };
					result = data.content;
					isErr  = data.is_error;
				} catch (e) {
					result = `MCP call failed: ${(e as Error).message}`;
					isErr  = true;
				}
			}
		} catch (e) { result = `Error: ${(e as Error).message}`; isErr = true; }
		return { result, isErr };
	}

	interface XmlCommand { name: string; path: string; content: string; mcpArgs?: string; }

	/**
	 * Parse XML-style commands from model output.
	 * Recognises:
	 *   <write_file path="...">content</write_file>
	 *   <read_file path="..." />  or  <read_file path="..."></read_file>
	 *   <list_directory path="..." />
	 *   <mcp_call name="...">{ "arg": "val" }</mcp_call>
	 */
	function _parseXmlCommands(text: string): XmlCommand[] {
		const cmds: XmlCommand[] = [];
		// write_file with body
		const writeRe = /<write_file\s+path="([^"]+)"\s*>([\s\S]*?)<\/write_file>/g;
		let m: RegExpExecArray | null;
		while ((m = writeRe.exec(text)) !== null) cmds.push({ name: 'write_file', path: m[1], content: m[2] });
		// read_file self-closing or empty
		const readRe = /<read_file\s+path="([^"]+)"\s*\/?>/g;
		while ((m = readRe.exec(text)) !== null) cmds.push({ name: 'read_file', path: m[1], content: '' });
		// list_directory
		const listRe = /<list_directory\s+path="([^"]+)"\s*\/?>/g;
		while ((m = listRe.exec(text)) !== null) cmds.push({ name: 'list_directory', path: m[1], content: '' });
		// mcp_call with JSON body
		const mcpRe = /<mcp_call\s+name="([^"]+)"\s*>([\s\S]*?)<\/mcp_call>/g;
		while ((m = mcpRe.exec(text)) !== null) cmds.push({ name: 'mcp_call', path: '', content: '', mcpArgs: m[2].trim(), mcpName: m[1] } as XmlCommand & { mcpName: string });
		return cmds;
	}

	/** Strip XML command tags from the visible message text. */
	function _stripXmlCommands(text: string): string {
		return text
			.replace(/<write_file\s+path="[^"]+"\s*>[\s\S]*?<\/write_file>/g, '')
			.replace(/<read_file\s+path="[^"]+"\s*\/?>/g, '')
			.replace(/<list_directory\s+path="[^"]+"\s*\/?>/g, '')
			.replace(/<mcp_call\s+name="[^"]+"\s*>[\s\S]*?<\/mcp_call>/g, '')
			.trim();
	}

	const MAX_AGENT_TURNS = 8;

	/**
	 * Execute one AI round-trip.
	 * - OpenAI: native function-calling for MCP tools in ALL modes; file tools added in agent mode.
	 * - Ollama/custom: XML mcp_call protocol in ALL modes; full XML agent protocol in agent mode.
	 */
	async function _doAiTurn(tidx: number, systemPrompt: string, depth: number, forceXml = false): Promise<void> {
		if (depth >= MAX_AGENT_TURNS) return;

		const isAgentMode = chatInteractionMode === 'agent';
		const enabledMcpTools = mcpTools.filter(t => t.enabled);

		// Anthropic uses a completely different API format (/v1/messages with its own tool schema).
		// Every other provider (openai, ollama, custom) speaks OpenAI-compatible
		// /v1/chat/completions with the `tools` field — modern Ollama supports this natively.
		// Use native function-calling for all non-Anthropic providers whenever any tools exist.
		// forceXml=true is set automatically when the model reports it does not support tools.
		const useNativeTools = !forceXml && settings.ai.provider !== 'anthropic' && (isAgentMode || enabledMcpTools.length > 0);
		// Non-OpenAI agent mode: full XML protocol (files + mcp_call).
		const useXmlAgent = isAgentMode && !useNativeTools;
		// Non-OpenAI non-agent with MCP tools: parse mcp_call XML in the response.
		const useXmlMcp = !useNativeTools && enabledMcpTools.length > 0;

		// Append empty assistant placeholder
		chatThreads[tidx].messages = [...chatThreads[tidx].messages, { role: 'assistant', content: '' }];
		const idx = chatThreads[tidx].messages.length - 1;

		const apiMessages = _buildApiMessages(chatThreads[tidx].messages.slice(0, -1), useXmlAgent);
		const reqBody: Record<string, unknown> = { ...settings.ai, messages: apiMessages, systemPrompt };
		if (useNativeTools) {
			// File-agent tools only in agent mode; MCP tools in every mode.
			const agentFileTools = isAgentMode ? AGENT_TOOLS : [];
			const mcpNativeTools = enabledMcpTools.map(t => ({
				type: 'function',
				function: { name: t.name, description: t.description, parameters: t.inputSchema },
			}));
			reqBody.tools = [...agentFileTools, ...mcpNativeTools];
			reqBody.toolChoice = 'auto';
		}

		const res = await fetch('/api/ai/chat', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(reqBody),
		});

		if (!res.ok) {
			const raw = await res.text().catch(() => '');
			let msg: string;
			try {
				const outer = JSON.parse(raw) as { error?: unknown };
				const errField = outer.error;
				if (typeof errField === 'string') {
					// ApiError wraps the upstream body as a string — try to extract the message
					try {
						const inner = JSON.parse(errField) as { error?: { message?: string } | string };
						const ie = inner.error;
						msg = (ie && typeof ie === 'object') ? (ie.message ?? errField) : (typeof ie === 'string' ? ie : errField);
					} catch { msg = errField; }
				} else if (errField && typeof errField === 'object') {
					msg = (errField as { message?: string }).message ?? JSON.stringify(errField);
				} else { msg = raw; }
			} catch { msg = raw; }
			if (!msg) msg = `HTTP ${res.status} ${res.statusText}`;
			// If the model doesn't support tools, transparently retry in XML mode
			if (useNativeTools && /does not support tools/i.test(msg)) {
				chatThreads[tidx].messages = chatThreads[tidx].messages.slice(0, -1);
				await _doAiTurn(tidx, systemPrompt, depth, true);
				return;
			}
			chatThreads[tidx].messages[idx] = { role: 'assistant', content: msg, error: true };
			return;
		}

		// ── Stream response ────────────────────────────────────────────────
		const reader = res.body!.getReader();
		const decoder = new TextDecoder();
		let buf = '';
		let hasNativeToolCalls = false;
		const pendingToolCalls: Record<number, { id: string; name: string; arguments: string }> = {};

		while (true) {
			const { done, value } = await reader.read();
			if (done) break;
			buf += decoder.decode(value, { stream: true });
			const lines = buf.split('\n');
			buf = lines.pop() ?? '';
			for (const line of lines) {
				if (!line.startsWith('data: ')) continue;
				const data = line.slice(6).trim();
				if (data === '[DONE]') continue;
				try {
					const p = JSON.parse(data) as {
						choices?: Array<{
							delta?: {
								content?: string;
								tool_calls?: Array<{ index?: number; id?: string; function?: { name?: string; arguments?: string } }>;
							};
							finish_reason?: string;
						}>;
					};
					const delta = p.choices?.[0]?.delta;
					if (!delta) continue;
					const chunk = delta.content ?? '';
					if (chunk) {
						chatThreads[tidx].messages[idx] = {
							...chatThreads[tidx].messages[idx],
							content: chatThreads[tidx].messages[idx].content + chunk,
						};
					}
					// Always accumulate native tool calls (dispatched below only when useNativeTools)
					if (delta.tool_calls) {
						for (const tc of delta.tool_calls) {
							const i = tc.index ?? 0;
							if (!pendingToolCalls[i]) pendingToolCalls[i] = { id: '', name: '', arguments: '' };
							if (tc.id) pendingToolCalls[i].id = tc.id;
							if (tc.function?.name) pendingToolCalls[i].name += tc.function.name;
							if (tc.function?.arguments) pendingToolCalls[i].arguments += tc.function.arguments;
						}
					}
					if (p.choices?.[0]?.finish_reason === 'tool_calls') hasNativeToolCalls = true;
				} catch { /* malformed chunk */ }
			}
		}

		// ── Native tool calls (OpenAI) ─────────────────────────────────────
		const nativeToolList = Object.values(pendingToolCalls);
		if (useNativeTools && (hasNativeToolCalls || nativeToolList.length > 0)) {
			const toolCallsArr = nativeToolList.map((tc) => ({
				id: tc.id || `call_${Date.now()}`,
				type: 'function' as const,
				function: { name: tc.name, arguments: tc.arguments },
			}));
			chatThreads[tidx].messages[idx] = {
				role: 'assistant',
				content: chatThreads[tidx].messages[idx].content,
				tool_calls: toolCallsArr,
			};
			for (const tc of toolCallsArr) {
				let args: Record<string, string> = {};
				try { args = JSON.parse(tc.function.arguments); } catch { /**/ }
				const { result, isErr } = await _execAgentAction(tc.function.name, args);
				chatThreads[tidx].messages = [...chatThreads[tidx].messages, {
					role: 'tool', tool_call_id: tc.id, tool_name: tc.function.name,
					content: result, error: isErr,
				}];
			}
			await _doAiTurn(tidx, systemPrompt, depth + 1);
			return;
		}

		// ── XML command protocol (Ollama / any model) ─────────────────────
		// Agent mode: full file + mcp_call protocol.
		// Non-agent with MCP: mcp_call only (no file commands in ask/plan mode).
		if (useXmlAgent || useXmlMcp) {
			const fullText = chatThreads[tidx].messages[idx].content;
			const cmds = useXmlAgent
				? _parseXmlCommands(fullText)
				: _parseXmlCommands(fullText).filter(c => c.name === 'mcp_call');
			if (cmds.length > 0) {
				// Show the clean text without XML tags
				chatThreads[tidx].messages[idx] = {
					...chatThreads[tidx].messages[idx],
					content: _stripXmlCommands(fullText),
				};
				let needsFollowUp = false;
				for (const cmd of cmds) {
					let result: string;
					let isErr: boolean;
					if (cmd.name === 'mcp_call') {
						// MCP tool call via XML protocol
						const mcpCmd = cmd as typeof cmd & { mcpName?: string; mcpArgs?: string };
						const toolName = mcpCmd.mcpName ?? '';
						let args: Record<string, unknown> = {};
						try { args = JSON.parse(mcpCmd.mcpArgs ?? '{}'); } catch { /**/ }
						try {
							const res = await fetch('/api/mcp/call', {
								method: 'POST',
								headers: { 'Content-Type': 'application/json' },
								body: JSON.stringify({ name: toolName, arguments: args }),
							});
							const data = await res.json() as { content: string; is_error: boolean };
							result = data.content; isErr = data.is_error;
						} catch (e) {
							result = `MCP call failed: ${(e as Error).message}`; isErr = true;
						}
						needsFollowUp = !isErr;
					} else {
						({ result, isErr } = await _execAgentAction(cmd.name, { path: cmd.path, content: cmd.content }));
						// If we read/listed, let the AI continue with the results
						if (cmd.name !== 'write_file' && !isErr) needsFollowUp = true;
					}
					chatThreads[tidx].messages = [...chatThreads[tidx].messages, {
						role: 'tool', tool_call_id: `xml_${Date.now()}`, tool_name: cmd.name,
						content: result, error: isErr,
					}];
				}
				if (needsFollowUp) await _doAiTurn(tidx, systemPrompt, depth + 1, forceXml);
			}
		}
	}

	async function sendChat(text: string) {
		const tidx = chatThreads.findIndex(t => t.id === activeChatId);
		if (tidx === -1) return;
		if (chatThreads[tidx].messages.length === 0) {
			chatThreads[tidx].title = text.length > 50 ? text.slice(0, 50) + '…' : text;
		}
		chatThreads[tidx].messages = [...chatThreads[tidx].messages, { role: 'user', content: text }];
		chatLoading = true;
		try {
			// Lazily load MCP tools if not yet fetched.
			if (mcpTools.length === 0) await fetchMcpTools();
			const enabledMcpTools = mcpTools.filter(t => t.enabled);

			let systemPrompt = 'You are a helpful coding assistant built into Nixium, a local code nixium. Be concise and practical. Format code in markdown triple-backtick blocks.';

			// Always advertise available MCP tools so the AI can use them in any mode.
			if (enabledMcpTools.length > 0) {
				const toolLines = enabledMcpTools.map(t => `- ${t.name}: ${t.description}`).join('\n');
				if (settings.ai.provider !== 'anthropic') {
					// Native tool calls: instruct the AI to call tools proactively rather than guessing.
					systemPrompt += `\n\nYou have access to the following tools and MUST call them when the user's question is relevant — do NOT guess or make up answers that a tool could provide:\n${toolLines}`;
				} else {
					// Anthropic / fallback: XML mcp_call protocol.
					systemPrompt += `\n\nYou have access to the following MCP tools. When the user's question is relevant, call the tool BEFORE answering by emitting:\n<mcp_call name="TOOL_NAME">{"arg":"value"}</mcp_call>\nDo NOT guess or make up answers that a tool could provide. Available tools:\n${toolLines}`;
				}
			}
			if (chatInteractionMode === 'plan') {
				systemPrompt += '\n\nThe user wants a PLAN: outline the approach in numbered steps before writing any code.';
			}
			if (chatInteractionMode === 'agent') {
				const mcpSection = enabledMcpTools.length > 0 && settings.ai.provider === 'anthropic'
					? `\n\nReminder — call MCP skills via XML too:\n` +
					  enabledMcpTools.map(t => `- ${t.name}`).join('\n') +
					  `\n\nSyntax: <mcp_call name="SKILL_NAME">{"arg": "value"}</mcp_call>`
					: '';
				// XML agent instructions are only needed for Anthropic; all other providers use
				// native function-calling tools for both file operations and MCP.
				if (settings.ai.provider === 'anthropic') {
					const xmlInstructions = `\n\nYou are in AGENT mode. You can read, write, and list files directly.

To write or create a file, output this XML in your response (the file will be saved immediately):
<write_file path="relative/path/to/file.py">
full file contents here
</write_file>

To read a file:
<read_file path="relative/path/to/file.py" />

To list a directory:
<list_directory path="." />${mcpSection}

RULES:
- When asked to create or save a file, ALWAYS use <write_file>. Never just show code in chat.
- Paths are relative to the open project root.
- You may use multiple commands in one response.
- After the XML block, briefly confirm what you did.`;
					systemPrompt += xmlInstructions;
				} else {
					systemPrompt += '\n\nYou are in AGENT mode. You can read, write, and list files and call tools. Use the tools provided to complete the task directly — do not just show code, actually execute the file operations.';
				}
			}
			if (chatUseContext && activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB) {
				systemPrompt += `\n\nThe user has this file open (${activeTab.name}):\n\`\`\`\n${activeTab.content.slice(0, 8000)}\n\`\`\``;
			}
			await _doAiTurn(tidx, systemPrompt, 0);
		} catch (err) {
			chatThreads[tidx].messages = [...chatThreads[tidx].messages, {
				role: 'assistant', content: (err as Error).message, error: true,
			}];
		} finally {
			chatLoading = false;
		}
	}

	// ── Settings ──────────────────────────────────────────────────────────────
	async function fetchOllamaModels(baseUrl?: string, updateDraft = true) {
		ollamaModelsLoading = true;
		ollamaModelsError = '';
		const url = baseUrl ?? (updateDraft ? settingsDraft.ai.baseUrl : settings.ai.baseUrl);
		try {
			const res = await fetch(`/api/ai/ollama-models?baseUrl=${encodeURIComponent(url || '')}`);
			if (!res.ok) {
				const e = await res.json().catch(() => ({ error: res.statusText }));
				ollamaModelsError = e.error ?? res.statusText;
				return;
			}
			const models: string[] = await res.json();
			ollamaModels = models;
			if (updateDraft && models.length > 0 && !models.includes(settingsDraft.ai.model)) {
				settingsDraft.ai.model = models[0];
			}
		} catch (e) {
			ollamaModelsError = (e as Error).message;
		} finally {
			ollamaModelsLoading = false;
		}
	}
	function openSettings() {
		settingsDraft = JSON.parse(JSON.stringify(settings));
		settingsOpen = true;
		menuOpen = false;
		if (settingsDraft.ai.provider === 'ollama') fetchOllamaModels();
	}
	function saveSettings() {
		settings = JSON.parse(JSON.stringify(settingsDraft));
		settingsOpen = false;
	}

	// ── Terminal ─────────────────────────────────────────────────────────────────
	function setTerminalVisible(v: boolean) {
		terminalVisible = v;
		if (v && terminalMode === 'tab') {
			activeTabPath = TERM_TAB;
			setTimeout(() => terminalRef?.focus(), 50);
		} else if (v && terminalMode === 'panel') {
			setTimeout(() => terminalRef?.focus(), 50);
		} else if (!v && activeTabPath === TERM_TAB) {
			const last = tabs[tabs.length - 1];
			if (last) activateTab(last.path); else activeTabPath = null;
		}
	}

	function toggleTerminal() { setTerminalVisible(!terminalVisible); }

	function setTerminalMode(mode: 'panel' | 'tab') {
		terminalMode = mode;
		if (mode === 'tab' && terminalVisible) {
			activeTabPath = TERM_TAB;
			setTimeout(() => terminalRef?.focus(), 50);
		} else if (mode === 'panel' && activeTabPath === TERM_TAB) {
			const last = tabs[tabs.length - 1];
			if (last) activateTab(last.path); else activeTabPath = null;
		}
	}

	// ── Run config ──────────────────────────────────────────────────────────────
	/** Parse a .nixium file and return the `run` command, or null if absent. */
	function parseEditorFile(text: string): { run?: string; name?: string } {
		const result: { run?: string; name?: string } = {};
		for (const raw of text.split('\n')) {
			const line = raw.replace(/#.*$/, '').trim();
			const m = line.match(/^(\w+)\s*=\s*(.+)$/);
			if (!m) continue;
			const key = m[1].toLowerCase();
			const val = m[2].trim().replace(/^["']|["']$/g, '');
			if (key === 'run') result.run = val;
			if (key === 'name') result.name = val;
		}
		return result;
	}

	async function runProject() {
		const cfgPath = rootPath.replace(/\/+$/, '') + '/.nixium';
		let command = '';
		try {
			const res = await fetch(`/api/fs/read?path=${encodeURIComponent(cfgPath)}`);
			if (res.ok) {
				const cfg = parseEditorFile(await res.text());
				command = cfg.run ?? '';
			} else if (res.status !== 404) {
				runMsg = { msg: `Could not read .nixium (${res.status})`, kind: 'error' };
				return;
			}
		} catch (err) {
			runMsg = { msg: `Network error reading .nixium: ${(err as Error).message}`, kind: 'error' };
			return;
		}
		if (!command) {
			runMsg = { msg: 'No run command — add  run = <cmd>  to .nixium in the workspace root', kind: 'info' };
			return;
		}
		runMsg = null;
		const alreadyOpen = terminalVisible;
		if (!alreadyOpen) setTerminalVisible(true);
		setTimeout(() => terminalRef?.sendText(`cd ${JSON.stringify(rootPath)} && ${command}\n`), alreadyOpen ? 0 : 200);
	}

	// ── Open Folder ───────────────────────────────────────────────────────────
	function openFolderPrompt() {
		menuOpen = false;
		openFolderMode = true;
	}

	function selectFolder(path: string) {
		rootPath = path;
		localStorage.setItem(ROOT_KEY, path);
		// Prepend to recents, deduplicate, cap at MAX_RECENT
		const next = [path, ...recentFolders.filter((p) => p !== path)].slice(0, MAX_RECENT);
		recentFolders = next;
		saveRecent(next);
		sidebarVisible = true;
		openFolderMode = false;
	}

	// ── Resize ────────────────────────────────────────────────────────────────
	function onMouseMove(e: MouseEvent) {
		if (isDragging) sidebarWidth = Math.max(140, Math.min(500, e.clientX));
		if (isTermDragging) {
			const bodyH = document.documentElement.clientHeight;
			terminalHeight = Math.max(80, Math.min(bodyH - 120, bodyH - e.clientY));
		}
		if (isChatDragging) {
			const bodyW = document.documentElement.clientWidth;
			chatWidth = Math.max(240, Math.min(700, bodyW - e.clientX));
		}
	}
	function onMouseUp() { isDragging = false; isTermDragging = false; isChatDragging = false; }

	// ── Keyboard shortcuts ────────────────────────────────────────────────────
	function handleKeydown(e: KeyboardEvent) {
		const mod = e.ctrlKey || e.metaKey;
		// Ctrl+A guard: prevent the browser's page-wide "select all" when focus sits on
		// chrome (toolbar buttons, tabs, sidebar items, status bar pieces – i.e. things
		// that are NOT content the user intends to copy).  We still allow Ctrl+A when:
		//   • focus is inside a real text input / textarea (native select-all works)
		//   • focus is inside the chat messages area (user wants to copy a response)
		//   • focus is inside a code block / pre (user wants to copy code)
		//   • CodeMirror editor: it captures Ctrl+A itself and never lets it bubble here
		if (mod && e.key === 'a') {
			const t = e.target as HTMLElement;
			const isTextInput = t instanceof HTMLInputElement
				|| t instanceof HTMLTextAreaElement
				|| t.isContentEditable;
			const isContent = !!t.closest?.('.chat-messages, .msg-body, .code-pre, .fif-results');
			if (!isTextInput && !isContent) e.preventDefault();
		}
		if (mod && e.key === 's') { e.preventDefault(); saveActiveTab(); }
		if (mod && e.key === 'b') { e.preventDefault(); sidebarVisible = !sidebarVisible; }
		if (mod && e.key === 'w') { e.preventDefault(); if (activeTabPath) closeTab(activeTabPath); }
		if (mod && e.key === '`') { e.preventDefault(); toggleTerminal(); }
		if (mod && e.key === 'Enter') { e.preventDefault(); runProject(); }
		if (mod && e.key === 'j') { e.preventDefault(); statusBarVisible = !statusBarVisible; }
		if (mod && e.key === 'k') { e.preventDefault(); toggleChat(); }
		// Ctrl+Shift+F – Find in Files
		if (mod && e.shiftKey && e.key === 'F') { e.preventDefault(); openFindInFiles(); }
		// Ctrl+Shift+X – Extensions
		if (mod && e.shiftKey && e.key === 'X') { e.preventDefault(); extOpen ? (extOpen = false) : openExtensions(); }
		// Ctrl+Shift+M – MCP Skills
		if (mod && e.shiftKey && e.key === 'M') { e.preventDefault(); mcpOpen ? (mcpOpen = false) : openMcp(); }
		// Ctrl+Shift+P – Command Palette
		if (mod && e.shiftKey && e.key === 'P') { e.preventDefault(); paletteOpen = !paletteOpen; }
		if (e.key === 'Escape') {
			if (paletteOpen) { paletteOpen = false; }
			else if (settingsOpen) { settingsOpen = false; }
			else if (saveAsOpen) { saveAsOpen = false; }
			else if (newFileOpen) { newFileOpen = false; }
			else if (fifOpen) { fifOpen = false; }
			else if (extOpen) { extOpen = false; }
			else if (mcpOpen) { mcpOpen = false; }
			else if (openFolderMode) { openFolderMode = false; }
			else if (menuOpen) { menuOpen = false; }
		}
	}
</script>

<svelte:head>
	<title>{activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB ? `${activeTab.name}${activeTab.dirty ? ' •' : ''} – Nixium` : 'Nixium'}</title>
</svelte:head>

<svelte:window onmousemove={onMouseMove} onmouseup={onMouseUp} onkeydown={handleKeydown} />

{#if notification}
	<div class="notification notification-{notification.type}" role="alert">
		{notification.msg}
		<button class="notif-close" onclick={() => (notification = null)}>×</button>
	</div>
{/if}

{#if openFolderMode}
	<FolderPicker
		initialPath={rootPath}
		onselect={selectFolder}
		oncancel={() => (openFolderMode = false)}
	/>
{/if}

{#if paletteOpen}
<CommandPalette
	commands={paletteCommands}
	onrun={() => (paletteOpen = false)}
	onclose={() => (paletteOpen = false)}
	onjumpline={(n) => nixium.jumpToLine(n, 0)}
/>
{/if}

<div class="shell" class:dragging={isDragging || isTermDragging || isChatDragging}>

{#if settingsOpen}
<SettingsModal
	bind:draft={settingsDraft}
	{ollamaModels}
	{ollamaModelsLoading}
	{ollamaModelsError}
	onsave={saveSettings}
	oncancel={() => (settingsOpen = false)}
	onfetchollama={fetchOllamaModels}
/>
{/if}

{#if saveAsOpen}
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="modal-backdrop" role="presentation" onmousedown={() => (saveAsOpen = false)}>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div class="modal modal-sm" role="dialog" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
		<div class="modal-title">💾 Save As</div>
		<label class="modal-label">Path
			<input type="text" bind:value={saveAsDraft} class="modal-input modal-mono" placeholder="/path/to/file.ext"
				onkeydown={(e) => { if (e.key === 'Enter') confirmSaveAs(); }} />
		</label>
		<div class="modal-actions">
			<button class="modal-btn" onclick={() => (saveAsOpen = false)}>Cancel</button>
			<button class="modal-btn primary" onclick={confirmSaveAs}>Save</button>
		</div>
	</div>
</div>
{/if}

{#if newFileOpen}
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="modal-backdrop" role="presentation" onmousedown={() => (newFileOpen = false)}>
	<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
	<div class="modal modal-sm" role="dialog" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
		<div class="modal-title">📄 New File</div>
		<label class="modal-label">Path
			<input type="text" bind:value={newFileDraft} class="modal-input modal-mono" placeholder="/path/to/file.ext"
				onkeydown={(e) => { if (e.key === 'Enter') confirmNewFile(); }} />
		</label>
		<div class="modal-actions">
			<button class="modal-btn" onclick={() => (newFileOpen = false)}>Cancel</button>
			<button class="modal-btn primary" onclick={confirmNewFile}>Create</button>
		</div>
	</div>
</div>
{/if}

	<header class="toolbar">
		<!-- File menu: uses click-outside action so z-index stacking contexts don't matter -->
		<div class="menu-wrap" use:clickOutside={() => (menuOpen = false)}>
			<button class="icon-btn menu-btn" onclick={() => (menuOpen = !menuOpen)} aria-haspopup="true" aria-expanded={menuOpen}>
				File ▾
			</button>
			{#if menuOpen}
				<ul class="dropdown" role="menu">
					<li role="menuitem">
						<button onclick={openNewFileModal}>📄 New File…</button>
					</li>
					<li role="menuitem">
						<button onclick={openFolderPrompt}>📂 Open Folder…</button>
					</li>
					{#if recentFolders.length > 0}
						<li class="menu-sep" role="separator"></li>
						<li class="menu-label" role="presentation">Recent</li>
						{#each recentFolders as recent}
							<li role="menuitem">
								<button
									class:active-folder={recent === rootPath}
									onclick={() => { menuOpen = false; selectFolder(recent); }}
									title={recent}
								>
									<span class="recent-icon">{recent === rootPath ? '📂' : '📁'}</span>
									<span class="recent-path">{recent}</span>
								</button>
							</li>
						{/each}
					{/if}
					<li class="menu-sep" role="separator"></li>
					<li role="menuitem">
						<button disabled={!activeTab} onclick={() => { menuOpen = false; saveActiveTab(); }}>
							💾 Save <kbd>Ctrl+S</kbd>
						</button>
					</li>
					<li role="menuitem">
						<button disabled={!activeTab || activeTabPath === TERM_TAB || activeTabPath === CHAT_TAB} onclick={openSaveAs}>
							💾 Save As…
						</button>
					</li>
					<li role="menuitem">
						<button onclick={() => { menuOpen = false; autosave = !autosave; }}>
							{autosave ? '✅' : '⬜'} Autosave
						</button>
					</li>
					<li role="menuitem">
						<button disabled={!activeTabPath || activeTabPath === TERM_TAB} onclick={() => { menuOpen = false; if (activeTabPath) closeTab(activeTabPath); }}>
							✕ Close Tab <kbd>Ctrl+W</kbd>
						</button>
					</li>
					<li class="menu-sep" role="separator"></li>
					<li role="menuitem">
						<button onclick={() => { menuOpen = false; toggleTerminal(); }}>
							{terminalVisible ? '⊟ Hide Terminal' : '⊞ Open Terminal'} <kbd>Ctrl+`</kbd>
						</button>
					</li>
					<li role="menuitem">
						<button onclick={() => { menuOpen = false; if (!terminalVisible) setTerminalVisible(true); setTerminalMode(terminalMode === 'tab' ? 'panel' : 'tab'); }}>
							{terminalMode === 'tab' ? '⊟ Terminal → Panel' : '⊞ Terminal → Tab'}
						</button>
					</li>
					<li class="menu-sep" role="separator"></li>
					<li role="menuitem">
						<button onclick={() => { menuOpen = false; statusBarVisible = !statusBarVisible; }}>
							{statusBarVisible ? '☑ Status Bar' : '☐ Status Bar'} <kbd>Ctrl+J</kbd>
						</button>
					</li>
					<li role="menuitem">
						<button onclick={() => { menuOpen = false; openFindInFiles(); }}>
							🔍 Find in Files <kbd>Ctrl+Shift+F</kbd>
						</button>
					</li>
					<li role="menuitem">
						<button onclick={() => { menuOpen = false; openExtensions(); }}>
							🧩 Extensions <kbd>Ctrl+Shift+X</kbd>
						</button>
					</li>
					<li role="menuitem">
						<button onclick={openMcp}>
							🔌 MCP Skills <kbd>Ctrl+Shift+M</kbd>
						</button>
					</li>
				</ul>
			{/if}
		</div>

		<button class="icon-btn" onclick={() => (sidebarVisible = !sidebarVisible)} title="Toggle Explorer (Ctrl+B)">
			{sidebarVisible ? '◧' : '▭'}
		</button>
		<button class="icon-btn ai-btn" onclick={toggleChat} title="AI Chat (Ctrl+K)">✦ AI</button>
		<button class="icon-btn" onclick={openFindInFiles} title="Find in Files (Ctrl+Shift+F)">⌕</button>
		<button class="icon-btn" onclick={openExtensions} title="Extensions (Ctrl+Shift+X)">⊞</button>
		<button class="icon-btn mcp-btn" class:mcp-btn-active={mcpOpen} onclick={openMcp} title="MCP Skills (Ctrl+Shift+M)">🔌</button>

		<div class="tabs" role="tablist">
			{#each tabs as tab (tab.path)}
				<div
					role="tab"
					class="tab"
					class:active={tab.path === activeTabPath}
					aria-selected={tab.path === activeTabPath}
					title={tab.path}
					tabindex="0"
					onclick={() => activateTab(tab.path)}
					onkeydown={(e) => e.key === 'Enter' && activateTab(tab.path)}
				>
					<span class="tab-name">{tab.name}</span>
					{#if tab.dirty && !autosave}<span class="tab-dot" aria-hidden="true"></span>{/if}
					{#if tab.dirty && autosave}<span class="tab-dot tab-dot-as" title="Autosave pending" aria-hidden="true"></span>{/if}
					<button
						class="tab-close"
						onclick={(e) => { e.stopPropagation(); closeTab(tab.path); }}
						tabindex="-1"
						aria-label="Close {tab.name}"
					>×</button>
				</div>
			{/each}
			{#if terminalMode === 'tab' && terminalVisible}
				<div
					role="tab"
					class="tab tab-terminal"
					class:active={activeTabPath === TERM_TAB}
					aria-selected={activeTabPath === TERM_TAB}
					tabindex="0"
					onclick={() => activateTab(TERM_TAB)}
					onkeydown={(e) => e.key === 'Enter' && activateTab(TERM_TAB)}
				>
					<span class="tab-name">⬛ Terminal</span>
					<button class="tab-close" onclick={(e) => { e.stopPropagation(); setTerminalVisible(false); }} tabindex="-1" aria-label="Close terminal">×</button>
				</div>
			{/if}
			{#if chatMode === 'tab' && chatVisible}
				<div
					role="tab"
					class="tab tab-chat"
					class:active={activeTabPath === CHAT_TAB}
					aria-selected={activeTabPath === CHAT_TAB}
					tabindex="0"
					onclick={() => activateTab(CHAT_TAB)}
					onkeydown={(e) => e.key === 'Enter' && activateTab(CHAT_TAB)}
				>
					<span class="tab-name">✦ AI</span>
					<button class="tab-close" onclick={(e) => { e.stopPropagation(); setChatVisible(false); }} tabindex="-1" aria-label="Close AI chat">×</button>
				</div>
			{/if}
		</div>

		<div class="toolbar-right">
			{#if autosave}<span class="autosave-badge" title="Autosave on">⚡</span>{/if}
			<button
				class="run-btn"
				onclick={runProject}
				title="Run project (Ctrl+Enter) — configure via .nixium in workspace root"
			>▶ Run</button>
			<button class="icon-btn save-btn" onclick={saveActiveTab} disabled={!activeTab || activeTabPath === TERM_TAB || activeTabPath === CHAT_TAB} title="Save (Ctrl+S)">💾</button>
			<button class="icon-btn" onclick={openSettings} title="Settings">⚙</button>
		</div>
	</header>

	<div class="body">

		<aside class="sidebar" class:sidebar-hidden={!sidebarVisible} style="width: {sidebarWidth}px" aria-hidden={!sidebarVisible}>
			{#if fifOpen}
				<FindInFilesPanel rootPath={rootPath} onjump={jumpToSearchResult} onclose={() => (fifOpen = false)} />
			{:else if extOpen}
				<ExtensionsPanel
					extList={extList}
					enabledExtensions={settings.extensions}
					extDetailName={extDetailName}
					installingExt={installingExt}
					onopendetail={openExtDetail}
					onopenstoredetail={openStoreExtDetail}
					ontoggle={(name, enabled) => { settings.extensions = { ...settings.extensions, [name]: enabled }; }}
					oninstall={installExtension}
					onclose={() => (extOpen = false)}
				/>
			{:else if mcpOpen}
				<McpPanel
					tools={mcpTools}
					loading={mcpToolsLoading}
					detailName={mcpDetailName}
					onclose={() => (mcpOpen = false)}
					onopendetail={openMcpDetail}
					ontoggle={toggleMcpTool}
				/>
			{:else}
				<FileBrowser {rootPath} activeFile={activeTabPath} onopen={openFile} />
			{/if}
		</aside>
		<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
		<div
			class="resize-handle"
			class:sidebar-hidden={!sidebarVisible}
			onmousedown={(e) => { isDragging = true; e.preventDefault(); }}
			role="separator"
			aria-orientation="vertical"
		></div>

		<div class="nixium-layout">
		<div class="nixium-column">
			<!-- Extension detail view (VS Code-style full-pane readme) -->
			<!-- MCP skill detail view -->
			{#if mcpDetailName}
				{@const mcpDetail = mcpTools.find(t => t.name === mcpDetailName)}
				<div class="ext-detail">
					<div class="ext-detail-topbar">
						<button class="ext-detail-back icon-btn" onclick={closeMcpDetail} title="Back to MCP list">← MCP Skills</button>
					</div>
					<div class="ext-detail-scroll">
						<div class="ext-detail-hero">
							<div class="ext-detail-icon">🔌</div>
							<div class="ext-detail-hero-info">
								<h1 class="ext-detail-title">{mcpDetail?.displayName ?? mcpDetailName}</h1>
								<div class="ext-detail-meta">
									<span class="ext-detail-author">Built-in MCP skill</span>
								</div>
								{#if mcpDetail?.description}<p class="ext-detail-desc">{mcpDetail.description}</p>{/if}
								<div class="ext-detail-actions">
									<label class="ext-detail-toggle-label">
										<input
											type="checkbox"
											checked={mcpDetail?.enabled ?? false}
											onchange={() => mcpDetailName && toggleMcpTool(mcpDetailName)}
										/>
										{mcpDetail?.enabled ? 'Enabled' : 'Disabled'}
									</label>
								</div>
							</div>
						</div>
						<hr class="ext-detail-hr" />
						<div class="ext-detail-body">
							{#if mcpDetailReadmeLoading}
								<div class="ext-detail-loading">Loading readme…</div>
							{:else if mcpDetailReadmeHtml}
								<!-- eslint-disable-next-line svelte/no-at-html-tags -->
								{@html mcpDetailReadmeHtml}
							{:else}
								<div class="ext-detail-no-readme">No README found.</div>
							{/if}
						</div>
					</div>
				</div>
			{/if}
			{#if extDetailName}
				{@const detailInstalled = extList.find(e => e.name === extDetailName)}
				{@const detailStore = extDetailStoreEntry}
				{@const detailInfo = detailInstalled
					? { displayName: detailInstalled.displayName, version: detailInstalled.version, description: detailInstalled.description, author: '' }
					: detailStore
					? { displayName: detailStore.displayName, version: detailStore.version, description: detailStore.description, author: detailStore.author ?? '' }
					: { displayName: extDetailName, version: '', description: '', author: '' }}
				<div class="ext-detail">
					<div class="ext-detail-topbar">
						<button class="ext-detail-back icon-btn" onclick={closeExtDetail} title="Back to list">← Extensions</button>
					</div>
					<div class="ext-detail-scroll">
						<div class="ext-detail-hero">
							<div class="ext-detail-icon">⊞</div>
							<div class="ext-detail-hero-info">
								<h1 class="ext-detail-title">{detailInfo.displayName}</h1>
								<div class="ext-detail-meta">
									{#if detailInfo.version}<span class="ext-detail-ver">v{detailInfo.version}</span>{/if}
									{#if detailInfo.author}<span class="ext-detail-author">{detailInfo.author}</span>{/if}
								</div>
								{#if detailInfo.description}<p class="ext-detail-desc">{detailInfo.description}</p>{/if}
								<!-- Actions -->
								<div class="ext-detail-actions">
									{#if detailInstalled}
										<!-- Enable / Disable toggle -->
										<label class="ext-detail-toggle-label">
											<input
												type="checkbox"
												checked={settings.extensions[extDetailName] ?? false}
												onchange={(e) => { settings.extensions = { ...settings.extensions, [extDetailName!]: (e.target as HTMLInputElement).checked }; }}
											/>
											{settings.extensions[extDetailName] ? 'Enabled' : 'Disabled'}
										</label>
										<!-- Uninstall -->
										<button class="ext-detail-btn ext-detail-btn-danger"
											disabled={removingExt === extDetailName}
											onclick={() => removeExtension(extDetailName!)}
										>{ removingExt === extDetailName ? 'Removing…' : 'Uninstall' }</button>
									{:else if detailStore}
										<!-- Install from store (only if we have a download URL) -->
										{#if detailStore.download_url}
											<button class="ext-detail-btn ext-detail-btn-primary"
												disabled={installingExt === extDetailName}
												onclick={async () => { await installExtension(detailStore); closeExtDetail(); }}
											>{ installingExt === extDetailName ? 'Installing…' : 'Install' }</button>
										{:else}
											<span class="ext-detail-local-badge">Local extension</span>
										{/if}
									{/if}
								</div>
							</div>
						</div>
						<hr class="ext-detail-hr" />
						<div class="ext-detail-body">
							{#if extDetailReadmeLoading}
								<div class="ext-detail-loading">Loading readme…</div>
							{:else if extDetailReadmeHtml}
								<!-- eslint-disable-next-line svelte/no-at-html-tags -->
								{@html extDetailReadmeHtml}
							{:else}
								<div class="ext-detail-no-readme">No README found.</div>
							{/if}
						</div>
					</div>
				</div>
			{/if}
			<main class="nixium-area" class:hidden={hideEditor}>
				{#if activeTab}
					<div class="nixium-host" use:nixium.action></div>
				{:else}
					<div class="welcome">
						<div class="welcome-inner">
							<div class="welcome-icon">✦</div>
							<p>Open a folder via <strong>File ▾ → Open Folder…</strong></p>
							<p class="hint">Ctrl+B · explorer · Ctrl+S · save · Ctrl+W · close · Ctrl+` · terminal · Ctrl+↵ · run</p>
						</div>
					</div>
				{/if}
			</main>

			{#if terminalMode === 'panel' && terminalVisible}
				<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
				<div
					class="term-resize-handle"
					onmousedown={(e) => { isTermDragging = true; e.preventDefault(); }}
					role="separator"
					aria-orientation="horizontal"
				></div>
			{/if}

			<!-- Terminal is always mounted to preserve PTY state; CSS controls layout -->
			<div
				class="term-host"
				class:term-panel={terminalMode === 'panel' && terminalVisible}
				class:term-in-tab={isTermTab}
				class:term-hidden={!terminalVisible}
				style={terminalMode === 'panel' && terminalVisible ? `height: ${terminalHeight}px` : ''}
			>
				<div class="term-bar">
					<span class="term-label">TERMINAL</span>
					<div class="term-bar-actions">
						<button class="icon-btn term-act" title={terminalMode === 'tab' ? 'Move to panel' : 'Move to tab'}
							onclick={() => setTerminalMode(terminalMode === 'tab' ? 'panel' : 'tab')}>
							{terminalMode === 'tab' ? '⊟' : '⊞'}
						</button>
						<button class="icon-btn term-act" onclick={() => setTerminalVisible(false)} title="Close (Ctrl+`)">×</button>
					</div>
				</div>
				<div class="term-body">
					<Terminal bind:this={terminalRef} cwd={rootPath} />
				</div>
			</div>
		</div>
		<!-- Chat panel (right side, panel mode) -->
		{#if chatMode === 'panel' && chatVisible}
			<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
			<div
				class="chat-vsep"
				onmousedown={(e) => { isChatDragging = true; e.preventDefault(); }}
				role="separator"
				aria-orientation="vertical"
			></div>
			<div class="chat-panel-host" style="width:{chatWidth}px">
				<Chat
					messages={chatThreads.find(t => t.id === activeChatId)?.messages ?? []}
					threads={chatThreads}
					{activeChatId}
					loading={chatLoading}
					activeFile={activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB ? activeTabPath : null}
					useContext={chatUseContext}
					mode="panel"
					interactionMode={chatInteractionMode}
					model={settings.ai.model}
					{ollamaModels}
					onsend={sendChat}
					onnewchat={newChat}
					onswitchchat={switchChat}
					onclose={() => setChatVisible(false)}
					onmovetotab={() => setChatMode('tab')}
					onmovetopanel={() => setChatMode('panel')}
					ontogglecontext={() => (chatUseContext = !chatUseContext)}
					onchangemode={(m) => (chatInteractionMode = m)}
					onchangemodel={(m) => (settings.ai.model = m)}
				/>
			</div>
		{/if}
		<!-- Chat tab (full area, tab mode) -->
		{#if isChatTab}
			<div class="chat-tab-host">
				<Chat
					messages={chatThreads.find(t => t.id === activeChatId)?.messages ?? []}
					threads={chatThreads}
					{activeChatId}
					loading={chatLoading}
					activeFile={null}
					useContext={chatUseContext}
					mode="tab"
					interactionMode={chatInteractionMode}
					model={settings.ai.model}
					{ollamaModels}
					onsend={sendChat}
					onnewchat={newChat}
					onswitchchat={switchChat}
					onclose={() => setChatVisible(false)}
					onmovetotab={() => setChatMode('tab')}
					onmovetopanel={() => setChatMode('panel')}
					ontogglecontext={() => (chatUseContext = !chatUseContext)}
					onchangemode={(m) => (chatInteractionMode = m)}
					onchangemodel={(m) => (settings.ai.model = m)}
				/>
			</div>
		{/if}
		</div>
	</div>

	{#if statusBarVisible}
		<div class="statusbar">
			<span class="statusbar-left">
				{#if runMsg}
					<span class="sb-msg" class:sb-error={runMsg.kind === 'error'} class:sb-info={runMsg.kind === 'info'} class:sb-success={runMsg.kind === 'success'}>
						{runMsg.msg}
					</span>
				{:else if activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB}
					{@const s = status(activeTab.path)}
					{#if s.msg}<span class="sb-msg" class:sb-error={s.kind === 'error'} class:sb-info={s.kind === 'info'} class:sb-success={s.kind === 'success'}>{s.msg}</span>{/if}
				{/if}
			</span>
			<span class="statusbar-right">
				{#if lineCount > 0}<span class="sb-lines">{lineCount} lines</span>{/if}
			</span>
		</div>
	{/if}
</div>

<style>
	:global(*) { box-sizing: border-box; margin: 0; padding: 0; }
	:global(html, body) { height: 100%; overflow: hidden; overscroll-behavior: none; }

	:root {
		--bg: #1e1e2e; --sidebar-bg: #181825; --surface: #27273a;
		--border: #313244; --hover-bg: #313244; --active-bg: #45475a;
		--text: #cdd6f4; --muted: #6c7086; --accent: #89b4fa;
		--success: #a6e3a1; --error: #f38ba8; --info: #fab387; --warning: #f9e2af;
		--radius: 5px; --toolbar-h: 38px;
	}

	.shell { display: flex; flex-direction: column; height: 100dvh; background: var(--bg); color: var(--text); font-family: system-ui, sans-serif; }
	.shell.dragging { cursor: col-resize; user-select: none; }

	.toolbar { display: flex; align-items: center; gap: 4px; padding: 0 8px; height: var(--toolbar-h); flex: 0 0 auto; background: var(--surface); border-bottom: 1px solid var(--border); }

	.icon-btn { flex: 0 0 auto; background: none; border: none; cursor: pointer; color: var(--muted); font-size: 16px; padding: 4px 6px; border-radius: var(--radius); line-height: 1; transition: color .12s, background .12s; }
	.icon-btn:hover:not(:disabled) { color: var(--text); background: var(--hover-bg); }
	.icon-btn:disabled { opacity: .3; cursor: default; }
	.save-btn { font-size: 14px; }
	.toolbar-right { margin-left: auto; display: flex; align-items: center; gap: 6px; flex: 0 0 auto; }
	.run-btn { padding: 4px 12px; background: #40a02b; border: none; border-radius: var(--radius); color: #fff; font-size: 12px; font-weight: 700; cursor: pointer; white-space: nowrap; transition: background .12s; }
	.run-btn:hover { background: #50c03b; }
	.autosave-badge { font-size: 13px; color: var(--accent); user-select: none; }

	/* Dropdown uses position:fixed to escape any stacking context (no z-index battles) */
	.menu-wrap { position: relative; flex: 0 0 auto; }
	.menu-btn { font-size: 12px; padding: 4px 8px; letter-spacing: .03em; }
	.dropdown {
		position: fixed; top: var(--toolbar-h); left: 8px;
		background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius);
		padding: 4px 0; min-width: 210px; list-style: none; z-index: 1000;
		box-shadow: 0 8px 24px #00000088;
	}
	.dropdown li > button { display: flex; align-items: center; justify-content: space-between; width: 100%; padding: 7px 14px; background: none; border: none; color: var(--text); font-size: 13px; cursor: pointer; white-space: nowrap; gap: 8px; }
	.dropdown li > button:hover:not(:disabled) { background: var(--hover-bg); }
	.dropdown li > button:disabled { opacity: .4; cursor: default; }
	.dropdown kbd { font-size: 10px; color: var(--muted); border: 1px solid var(--border); border-radius: 3px; padding: 0 4px; line-height: 1.6; }
	.menu-sep { height: 1px; background: var(--border); margin: 4px 0; }
	.menu-label { padding: 4px 14px 2px; font-size: 10px; font-weight: 700; letter-spacing: .07em; color: var(--muted); text-transform: uppercase; cursor: default; }
	.dropdown .recent-path { overflow: hidden; text-overflow: ellipsis; max-width: 170px; font-family: 'JetBrains Mono', monospace; font-size: 11px; }
	.dropdown .recent-icon { flex: 0 0 auto; }
	.dropdown button.active-folder { color: var(--accent); }

	.tabs { display: flex; align-items: stretch; flex: 1; min-width: 0; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; }
	.tabs::-webkit-scrollbar { display: none; }
	.tab { display: flex; align-items: center; gap: 4px; padding: 0 10px; height: 100%; border: none; border-right: 1px solid var(--border); border-bottom: 2px solid transparent; background: transparent; color: var(--muted); font-size: 12.5px; cursor: pointer; white-space: nowrap; max-width: 180px; transition: background .1s, color .1s; }
	.tab:hover { background: var(--hover-bg); color: var(--text); }
	.tab.active { background: var(--bg); color: var(--text); border-bottom-color: var(--accent); }
	.tab-name { overflow: hidden; text-overflow: ellipsis; max-width: 120px; }
	.tab-dot { width: 6px; height: 6px; border-radius: 50%; background: var(--warning); flex: 0 0 auto; }
	.tab-dot-as { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); flex: 0 0 auto; }
	.tab-terminal.active { border-bottom-color: var(--success) !important; }
	.tab-chat.active { border-bottom-color: var(--accent) !important; }

	/* Editor layout */
	.nixium-layout { flex: 1 1 auto; display: flex; flex-direction: row; min-width: 0; overflow: hidden; }

	/* Chat panel (right side) */
	.chat-vsep { flex: 0 0 4px; cursor: col-resize; background: var(--border); transition: background .15s; }
	.chat-vsep:hover { background: var(--accent); }
	.chat-panel-host { flex: 0 0 auto; display: flex; flex-direction: column; overflow: hidden; border-left: 1px solid var(--border); }
	.chat-tab-host { flex: 1 1 auto; display: flex; flex-direction: column; overflow: hidden; }

	/* AI button */
	.ai-btn { color: var(--accent); font-weight: 700; font-size: 12px; }

	/* Settings modal */
	.modal-backdrop { position: fixed; inset: 0; z-index: 500; background: #00000088; display: flex; align-items: center; justify-content: center; }
	.modal { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 20px 24px; min-width: 380px; max-width: 500px; display: flex; flex-direction: column; gap: 14px; box-shadow: 0 20px 60px #00000099; }
	.modal-title { font-size: 14px; font-weight: 600; color: var(--text); }
	.modal-label { display: flex; flex-direction: column; gap: 5px; font-size: 12px; color: var(--muted); }
	.modal-input { padding: 7px 10px; background: var(--bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 13px; outline: none; width: 100%; }
	.modal-input:focus { border-color: var(--accent); }
	.modal-mono { font-family: 'JetBrains Mono', monospace; }
	.modal-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
	.modal-btn { padding: 6px 16px; background: var(--hover-bg); border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); font-size: 13px; cursor: pointer; }
	.modal-btn:hover { background: var(--active-bg); }
	.modal-btn.primary { background: var(--accent); color: var(--bg); border-color: var(--accent); font-weight: 600; }
	.modal-btn.primary:hover { filter: brightness(1.1); }
	.modal-sm { min-width: 320px; max-width: 420px; }
	.tab-close { flex: 0 0 auto; background: none; border: none; cursor: pointer; color: var(--muted); font-size: 14px; padding: 0 2px; border-radius: 3px; line-height: 1; opacity: 0; transition: opacity .1s, background .1s; }
	.tab:hover .tab-close, .tab.active .tab-close { opacity: 1; }
	.tab-close:hover { background: var(--hover-bg); color: var(--error); }

	.body { display: flex; flex: 1 1 auto; min-height: 0; overflow: hidden; }
	.sidebar { flex: 0 0 auto; min-width: 140px; max-width: 500px; border-right: 1px solid var(--border); overflow: hidden; display: flex; flex-direction: column; }
	.sidebar-hidden { display: none !important; }
	.resize-handle { flex: 0 0 4px; cursor: col-resize; background: transparent; transition: background .15s; }
	.resize-handle:hover, .shell.dragging .resize-handle { background: var(--accent); }

	.nixium-column { flex: 1 1 auto; min-width: 0; display: flex; flex-direction: column; overflow: hidden; }
	.nixium-area { flex: 1 1 auto; min-height: 0; overflow: hidden; display: flex; flex-direction: column; }
	.nixium-area.hidden { display: none !important; }
	.nixium-host { flex: 1 1 auto; min-height: 0; overflow: hidden; }
	:global(.nixium-host .cm-editor) { height: 100%; }
	:global(.nixium-host .cm-scroller) { overflow: auto; }

	.term-resize-handle { flex: 0 0 4px; cursor: row-resize; background: var(--border); transition: background .15s; }
	.term-resize-handle:hover, .shell.dragging .term-resize-handle { background: var(--accent); }
	/* Terminal host – always in DOM, CSS toggles layout */
	.term-host { display: none; flex-direction: column; overflow: hidden; background: #11111b; }
	.term-host.term-panel { display: flex; flex: 0 0 auto; border-top: 1px solid var(--border); }
	.term-host.term-in-tab { display: flex; flex: 1 1 auto; min-height: 0; }
	.term-bar { display: flex; align-items: center; justify-content: space-between; padding: 2px 8px; background: var(--sidebar-bg); border-bottom: 1px solid var(--border); flex: 0 0 auto; min-height: 26px; }
	.term-bar-actions { display: flex; align-items: center; gap: 2px; }
	.term-act { font-size: 13px; padding: 2px 5px; }
	.term-label { font-size: 11px; font-weight: 700; letter-spacing: .06em; color: var(--muted); }
	.term-body { flex: 1 1 auto; overflow: hidden; padding: 4px; }

	.welcome { flex: 1; display: flex; align-items: center; justify-content: center; color: var(--muted); }
	.welcome-inner { text-align: center; display: flex; flex-direction: column; gap: 12px; }
	.welcome-icon { font-size: 40px; color: var(--border); }
	.welcome p { font-size: 14px; }
	.hint { font-size: 11px !important; color: var(--border); }

	/* Status bar */
	.statusbar { flex: 0 0 auto; display: flex; align-items: center; justify-content: space-between; height: 22px; padding: 0 10px; background: var(--surface); border-top: 1px solid var(--border); font-size: 11px; color: var(--muted); user-select: none; overflow: hidden; }
	.statusbar-left { display: flex; align-items: center; gap: 8px; min-width: 0; overflow: hidden; }
	.statusbar-right { display: flex; align-items: center; gap: 12px; flex: 0 0 auto; }
	.sb-msg { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
	.sb-error { color: var(--error); }
	.sb-info { color: var(--info); }
	.sb-success { color: var(--success); }
	.sb-lines { color: var(--muted); }

	@media (max-width: 480px) {
		.sidebar { position: absolute; top: var(--toolbar-h); bottom: 0; left: 0; z-index: 20; box-shadow: 4px 0 16px #00000066; }
		.resize-handle { display: none; }
	}

	/* Extension detail pane (VS Code-style full-width readme view) */
	.ext-detail { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; background: var(--bg); }
	.ext-detail-topbar { flex: 0 0 auto; padding: 6px 12px; border-bottom: 1px solid var(--border); }
	.ext-detail-back { font-size: 12px !important; color: var(--accent) !important; padding: 2px 6px !important; }
	.ext-detail-scroll { flex: 1 1 auto; overflow-y: auto; padding: 24px 32px 40px; }
	.ext-detail-hero { display: flex; gap: 20px; align-items: flex-start; margin-bottom: 16px; }
	.ext-detail-icon { font-size: 56px; line-height: 1; flex: 0 0 auto; color: var(--accent); }
	.ext-detail-hero-info { flex: 1; display: flex; flex-direction: column; gap: 6px; }
	.ext-detail-title { margin: 0; font-size: 22px; font-weight: 700; color: var(--text); }
	.ext-detail-meta { display: flex; gap: 10px; flex-wrap: wrap; }
	.ext-detail-ver { font-size: 11px; padding: 1px 6px; border-radius: 10px; background: var(--surface); color: var(--muted); border: 1px solid var(--border); }
	.ext-detail-author { font-size: 12px; color: var(--muted); }
	.ext-detail-desc { margin: 0; font-size: 13px; color: var(--muted); line-height: 1.5; }
	.ext-detail-actions { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-top: 4px; }
	.ext-detail-btn { padding: 5px 14px; border-radius: 4px; font-size: 12px; font-weight: 600; border: none; cursor: pointer; transition: opacity .15s; }
	.ext-detail-btn:disabled { opacity: .45; cursor: default; }
	.ext-detail-btn-primary { background: var(--accent); color: #000; }
	.ext-detail-btn-primary:hover:not(:disabled) { opacity: .85; }
	.ext-detail-btn-danger { background: #5a1a1a; color: #f99; border: 1px solid #8a3a3a; }
	.ext-detail-btn-danger:hover:not(:disabled) { background: #7a2020; }
	.ext-detail-toggle-label { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text); cursor: pointer; }
	.ext-detail-toggle-label input { accent-color: var(--accent); width: 14px; height: 14px; cursor: pointer; }
	.ext-detail-hr { border: none; border-top: 1px solid var(--border); margin: 18px 0; }
	.ext-detail-body { font-size: 13px; color: var(--text); line-height: 1.7; max-width: 800px; }
	:global(.ext-detail-body h1) { font-size: 20px; margin: 1.2em 0 .4em; }
	:global(.ext-detail-body h2) { font-size: 16px; margin: 1em 0 .3em; }
	:global(.ext-detail-body h3) { font-size: 14px; margin: .8em 0 .2em; }
	:global(.ext-detail-body p)  { margin: .5em 0; }
	:global(.ext-detail-body code) { font-family: 'JetBrains Mono', monospace; font-size: 12px; background: var(--surface); padding: 1px 4px; border-radius: 3px; }
	:global(.ext-detail-body pre) { background: var(--surface); border-radius: 6px; padding: 12px 14px; overflow-x: auto; }
	:global(.ext-detail-body pre code) { background: none; padding: 0; }
	:global(.ext-detail-body a) { color: var(--accent); }
	:global(.ext-detail-body ul, .ext-detail-body ol) { padding-left: 1.5em; margin: .4em 0; }
	:global(.ext-detail-body li) { margin: .2em 0; }
	:global(.ext-detail-body img) { max-width: 100%; border-radius: 4px; }
	:global(.ext-detail-body blockquote) { border-left: 3px solid var(--accent); padding-left: 12px; margin: .5em 0; color: var(--muted); }
	.ext-detail-loading { color: var(--muted); font-size: 12px; }
	.ext-detail-no-readme { color: var(--muted); font-size: 12px; font-style: italic; }
	/* Notification toast */
	.notification { position: fixed; bottom: 36px; left: 50%; transform: translateX(-50%); z-index: 9999; display: flex; align-items: center; gap: 10px; padding: 9px 14px; border-radius: 6px; font-size: 13px; box-shadow: 0 4px 20px rgba(0,0,0,.45); white-space: nowrap; max-width: 80vw; overflow: hidden; text-overflow: ellipsis; }
	.notification-info { background: var(--surface); color: var(--text); border: 1px solid var(--border); }
	.notification-error { background: #5a1a1a; color: #f99; border: 1px solid #8a3a3a; }
	.notif-close { background: none; border: none; color: inherit; cursor: pointer; font-size: 17px; padding: 0 2px; line-height: 1; opacity: .7; flex-shrink: 0; }
	.notif-close:hover { opacity: 1; }

	/* MCP toolbar button active state */
	.mcp-btn { font-size: 14px; }
	.mcp-btn-active { color: var(--accent) !important; }
</style>
