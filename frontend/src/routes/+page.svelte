<script lang="ts">
	import { tick, onMount } from 'svelte';
	import { createCodeMirrorAction, type EditorExtensionKey } from '$lib/useCodeMirror';
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
	import type { ExtensionManifest } from '$lib/extensions';
	import { type AppSettings, type PaletteCommand, type Tab, type StatusKind, type McpToolInfo, type ExternalMcpServer, type ExternalMcpToolInfo, DEFAULT_SETTINGS, loadSettings, SETTINGS_KEY, ROOT_KEY, RECENT_KEY, AUTOSAVE_KEY, TERM_TAB, CHAT_TAB, MAX_RECENT, loadRecent, saveRecent } from '$lib/types';
	import { clickOutside } from '$lib/actions';


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
	let chatThreads     = $state<ChatThread[]>([]);
	let activeChatId    = $state(_initChatId);
	let chatLoading     = $state(false);
	let chatAbortController = $state<AbortController | null>(null);
	let queuedChat      = $state<string | null>(null);
	let chatWarning     = $state(false);
	let chatWarningTimer = $state<ReturnType<typeof setTimeout> | null>(null);

	$effect(() => {
		if (chatLoading) {
			const timeoutSecs = (settings.ai.timeoutSecs || 120);
			// Warn at half the timeout, clamped to 15–45 s, so the banner always
			// appears well before the backend hard-cutoff fires.
			const warnAfterMs = Math.min(45, Math.max(15, timeoutSecs * 0.5)) * 1000;
			chatWarningTimer = setTimeout(() => { chatWarning = true; }, warnAfterMs);
		} else {
			if (chatWarningTimer !== null) { clearTimeout(chatWarningTimer); chatWarningTimer = null; }
			chatWarning = false;
		}
		return () => { if (chatWarningTimer !== null) { clearTimeout(chatWarningTimer); chatWarningTimer = null; } };
	});
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

	// ── External MCP servers ──────────────────────────────────────────────────
	let externalServers         = $state<ExternalMcpServer[]>([]);
	let externalToolsByServer   = $state<Record<string, ExternalMcpToolInfo[]>>({});

	async function fetchExternalServers() {
		try {
			const res = await fetch('/api/mcp/external');
			if (res.ok) externalServers = await res.json();
		} catch { /* ignore */ }
	}

	async function addExternalServer(cfg: Omit<ExternalMcpServer, 'id' | 'enabled'>) {
		const res = await fetch('/api/mcp/external', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(cfg),
		});
		if (!res.ok) throw new Error(await res.text());
		const srv: ExternalMcpServer = await res.json();
		externalServers = [...externalServers, srv];
	}

	async function deleteExternalServer(id: string) {
		try {
			await fetch(`/api/mcp/external/${encodeURIComponent(id)}`, { method: 'DELETE' });
			externalServers = externalServers.filter(s => s.id !== id);
			const next = { ...externalToolsByServer };
			delete next[id];
			externalToolsByServer = next;
		} catch { /* ignore */ }
	}

	async function toggleExternalServer(id: string) {
		try {
			const res = await fetch(`/api/mcp/external/${encodeURIComponent(id)}/toggle`, { method: 'POST' });
			if (res.ok) {
				const updated: ExternalMcpServer = await res.json();
				externalServers = externalServers.map(s => s.id === id ? updated : s);
			}
		} catch { /* ignore */ }
	}

	async function fetchServerTools(id: string) {
		try {
			const res = await fetch(`/api/mcp/external/${encodeURIComponent(id)}/tools`);
			if (res.ok) {
				const tools: ExternalMcpToolInfo[] = await res.json();
				externalToolsByServer = { ...externalToolsByServer, [id]: tools };
			}
		} catch { /* ignore */ }
	}

	async function toggleServerTool(serverId: string, toolName: string) {
		try {
			const res = await fetch(`/api/mcp/external/${encodeURIComponent(serverId)}/tools/${encodeURIComponent(toolName)}/toggle`, { method: 'POST' });
			if (res.ok) {
				const tools: ExternalMcpToolInfo[] = await res.json();
				externalToolsByServer = { ...externalToolsByServer, [serverId]: tools };
			}
		} catch { /* ignore */ }
	}

	function openMcp() {
		mcpOpen    = true;
		fifOpen    = false;
		extOpen    = false;
		sidebarVisible = true;
		menuOpen   = false;
		if (mcpTools.length === 0) fetchMcpTools();
		if (externalServers.length === 0) fetchExternalServers();
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
	// Load chat threads from server on mount
	onMount(async () => {
		try {
			const res = await fetch('/api/chats');
			if (res.ok) {
				const saved: ChatThread[] = await res.json();
				if (saved.length > 0) {
					chatThreads = saved;
					activeChatId = saved[0].id;
				} else {
					chatThreads = [{ id: _initChatId, title: 'New Chat', messages: [], createdAt: Date.now() }];
				}
			} else {
				chatThreads = [{ id: _initChatId, title: 'New Chat', messages: [], createdAt: Date.now() }];
			}
		} catch {
			chatThreads = [{ id: _initChatId, title: 'New Chat', messages: [], createdAt: Date.now() }];
		}
	});
	function saveChatThreads() {
		fetch('/api/chats', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(chatThreads),
		}).catch(() => {});
	}
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
		saveChatThreads();
	}
	function switchChat(id: string) { activeChatId = id; }
	function deleteChat(id: string) {
		const remaining = chatThreads.filter(t => t.id !== id);
		if (remaining.length === 0) {
			const newId = _genChatId();
			chatThreads = [{ id: newId, title: 'New Chat', messages: [], createdAt: Date.now() }];
			activeChatId = newId;
		} else {
			chatThreads = remaining;
			if (activeChatId === id) activeChatId = remaining[0].id;
		}
		fetch(`/api/chats/${encodeURIComponent(id)}`, { method: 'DELETE' }).catch(() => {});
	}

	// session ID of the currently-running (or recently-completed) AI request.
	// Stored so the stop button can cancel it server-side and so reconnect
	// logic can resume from the right offset.
	let chatSessionId = $state<string | null>(null);

	function stopChat() {
		chatAbortController?.abort();
		chatAbortController = null;
		// Cancel server-side so the agent loop exits rather than running orphaned.
		if (chatSessionId) {
			fetch(`/api/ai/agent/${chatSessionId}`, { method: 'DELETE' }).catch(() => {});
			chatSessionId = null;
		}
	}

	function continueChat() {
		chatWarning = false;
		// Reset the warn timer so the banner reappears after another interval.
		if (chatWarningTimer !== null) { clearTimeout(chatWarningTimer); chatWarningTimer = null; }
		const timeoutSecs = (settings.ai.timeoutSecs || 120);
		const warnAfterMs = Math.min(45, Math.max(15, timeoutSecs * 0.5)) * 1000;
		chatWarningTimer = setTimeout(() => { chatWarning = true; }, warnAfterMs);
	}

	async function sendChat(text: string) {
		const tidx = chatThreads.findIndex(t => t.id === activeChatId);
		if (tidx === -1) return;
		if (chatThreads[tidx].messages.length === 0) {
			chatThreads[tidx].title = text.length > 50 ? text.slice(0, 50) + '…' : text;
		}
		chatThreads[tidx].messages = [...chatThreads[tidx].messages, { role: 'user', content: text }];
		chatLoading = true;
		const controller = new AbortController();
		chatAbortController = controller;

		// Shared state across the initial fetch and any reconnect attempts.
		let msgIdx: number | null = null;   // index of the current assistant message
		let gotDone = false;                // true once a 'done' event was received
		let receivedCount = 0;             // next expected event index (for resume)

		// Read an SSE response body, updating chatThreads reactively.
		// Persists msgIdx / gotDone / receivedCount across calls for reconnects.
		const readStream = async (response: Response) => {
			const reader = response.body!.getReader();
			const decoder = new TextDecoder();
			let buf = '';
			while (true) {
				const { done, value } = await reader.read();
				if (done) break;
				buf += decoder.decode(value, { stream: true });
				const lines = buf.split('\n');
				buf = lines.pop() ?? '';
				for (const line of lines) {
					// Track event index so we know where to resume from.
					if (line.startsWith('id: ')) {
						const id = parseInt(line.slice(4));
						if (!isNaN(id)) receivedCount = id + 1;
						continue;
					}
					if (!line.startsWith('data: ')) continue;
					try {
						const ev = JSON.parse(line.slice(6)) as { type: string; [k: string]: unknown };
						switch (ev.type) {
							case 'turn_start':
								chatThreads[tidx].messages = [...chatThreads[tidx].messages, { role: 'assistant', content: '' }];
								msgIdx = chatThreads[tidx].messages.length - 1;
								break;
							case 'turn_abort':
								chatThreads[tidx].messages = chatThreads[tidx].messages.slice(0, -1);
								msgIdx = null;
								break;
							case 'text':
								if (msgIdx !== null) {
									chatThreads[tidx].messages[msgIdx] = {
										...chatThreads[tidx].messages[msgIdx],
										content: chatThreads[tidx].messages[msgIdx].content + (ev.content as string),
									};
								}
								break;
							case 'text_set':
								if (msgIdx !== null) {
									chatThreads[tidx].messages[msgIdx] = {
										...chatThreads[tidx].messages[msgIdx],
										content: ev.content as string,
									};
								}
								break;
							case 'tool_call':
								if (msgIdx !== null) {
									const tc = { id: ev.id as string, type: 'function' as const, function: { name: ev.name as string, arguments: JSON.stringify(ev.args) } };
									const cur = chatThreads[tidx].messages[msgIdx];
									chatThreads[tidx].messages[msgIdx] = { ...cur, tool_calls: [...(cur.tool_calls ?? []), tc] };
								}
								break;
							case 'tool_result': {
								const fw = ev.file_written as { path: string; content: string } | undefined;
								if (fw) {
									const t = tabs.find(t => t.path === fw.path);
									if (t) {
										t.content = fw.content;
										t.dirty = false;
										if (activeTabPath === fw.path) { await tick(); nixium.setValue(fw.content); }
									}
								}
								chatThreads[tidx].messages = [...chatThreads[tidx].messages, {
									role: 'tool', tool_call_id: ev.id as string, tool_name: ev.name as string,
									content: ev.content as string, error: ev.is_error as boolean,
								}];
								break;
							}
							case 'error':
								if (msgIdx !== null) {
									chatThreads[tidx].messages[msgIdx] = { ...chatThreads[tidx].messages[msgIdx], content: ev.message as string, error: true };
								} else {
									chatThreads[tidx].messages = [...chatThreads[tidx].messages, { role: 'assistant', content: ev.message as string, error: true }];
								}
								break;
							case 'done':
								gotDone = true;
								break;
						}
					} catch { /* malformed event */ }
				}
			}
		};

		try {
			const reqBody: Record<string, unknown> = {
				...settings.ai,
				messages: chatThreads[tidx].messages,
				mode: chatInteractionMode,
				rootPath,
			};
			if (chatUseContext && activeTab && activeTabPath !== TERM_TAB && activeTabPath !== CHAT_TAB) {
				reqBody.contextFile = { name: activeTab.name, content: activeTab.content.slice(0, 8000) };
			}

			const res = await fetch('/api/ai/agent', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify(reqBody),
				signal: controller.signal,
			});

			if (!res.ok) {
				const msg = await res.text().catch(() => `HTTP ${res.status}`);
				chatThreads[tidx].messages = [...chatThreads[tidx].messages, { role: 'assistant', content: msg, error: true }];
				return;
			}

			chatSessionId = res.headers.get('x-session-id');

			// Unified reconnect loop.
			// First iteration consumes the initial response already in hand.
			// If the stream throws a network error (not user-abort), we wait and
			// hit the resume endpoint.  If the resume fetch itself fails (WiFi
			// still down), currentStream stays null and we retry again next lap.
			let currentStream: Response | null = res;
			let retryDelay = 1500;
			let retries = 0;
			const MAX_RETRIES = 10;

			while (!gotDone) {
				if (currentStream) {
					try {
						await readStream(currentStream);
						break; // stream ended cleanly
					} catch (streamErr) {
						if ((streamErr as DOMException).name === 'AbortError') throw streamErr;
						// Network dropped mid-stream — fall through to reconnect
					}
				}

				if (!chatSessionId || retries >= MAX_RETRIES) break;
				retries++;
				await new Promise(r => setTimeout(r, retryDelay));
				retryDelay = Math.min(retryDelay * 2, 15_000);

				if (controller.signal.aborted) throw new DOMException('Aborted', 'AbortError');

				try {
					const next = await fetch(
						`/api/ai/agent/stream/${chatSessionId}?from=${receivedCount}`,
						{ signal: controller.signal }
					);
					if (!next.ok) break; // session expired / server restarted
					currentStream = next;
				} catch (fetchErr) {
					if ((fetchErr as DOMException).name === 'AbortError') throw fetchErr;
					currentStream = null; // WiFi still down — retry the wait
				}
			}

		} catch (err) {
			if ((err as DOMException).name === 'AbortError') {
				// User pressed stop — server-side cancel is handled by stopChat().
			} else {
				chatThreads[tidx].messages = [...chatThreads[tidx].messages, {
					role: 'assistant', content: (err as Error).message, error: true,
				}];
			}
		} finally {
			chatSessionId = null;
			chatAbortController = null;
			chatLoading = false;
			saveChatThreads();
			// Auto-send any message that was queued while we were busy.
			// Use setTimeout(0) to defer past the current synchronous tick so Svelte
			// can process chatLoading = false before sendChat sets it back to true.
			if (queuedChat) {
				const q = queuedChat;
				queuedChat = null;
				setTimeout(() => sendChat(q), 0);
			}
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
					{externalServers}
					{externalToolsByServer}
					onclose={() => (mcpOpen = false)}
					onopendetail={openMcpDetail}
					ontoggle={toggleMcpTool}
					onaddserver={addExternalServer}
					ondeleteserver={deleteExternalServer}
					ontoggleserver={toggleExternalServer}
					onfetchservertools={fetchServerTools}
					ontoggleservertool={toggleServerTool}
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
					ondeletechat={deleteChat}
					onclose={() => setChatVisible(false)}
					onmovetotab={() => setChatMode('tab')}
					onmovetopanel={() => setChatMode('panel')}
					ontogglecontext={() => (chatUseContext = !chatUseContext)}
					onchangemode={(m) => (chatInteractionMode = m)}
					onchangemodel={(m) => (settings.ai.model = m)}
					onstop={stopChat}
					onqueue={(t) => (queuedChat = t)}
					queuedMessage={queuedChat}
					longRunning={chatWarning}
					oncontinue={continueChat}
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
					ondeletechat={deleteChat}
					onclose={() => setChatVisible(false)}
					onmovetotab={() => setChatMode('tab')}
					onmovetopanel={() => setChatMode('panel')}
					ontogglecontext={() => (chatUseContext = !chatUseContext)}
					onchangemode={(m) => (chatInteractionMode = m)}
					onchangemodel={(m) => (settings.ai.model = m)}
					onstop={stopChat}
					onqueue={(t) => (queuedChat = t)}
					queuedMessage={queuedChat}
					longRunning={chatWarning}
					oncontinue={continueChat}
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
