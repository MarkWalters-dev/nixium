import type { EditorExtensionKey } from '$lib/useCodeMirror';

// ── Editor tab ────────────────────────────────────────────────────────────────
export interface Tab { path: string; name: string; content: string; dirty: boolean; }
export type StatusKind = 'idle' | 'info' | 'success' | 'error';

// ── Storage keys & special tab sentinels ─────────────────────────────────────
export const ROOT_KEY     = 'nixium-root';
export const RECENT_KEY   = 'nixium-recent-folders';
export const AUTOSAVE_KEY = 'nixium-autosave';
export const TERM_TAB     = '__terminal__';
export const CHAT_TAB     = '__chat__';
export const MAX_RECENT   = 8;

export function loadRecent(): string[] {
	try { return JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]'); } catch { return []; }
}
export function saveRecent(list: string[]) {
	localStorage.setItem(RECENT_KEY, JSON.stringify(list));
}

// ── MCP tool ─────────────────────────────────────────────────────────────────
export interface McpToolInfo {
	name: string;
	displayName: string;
	description: string;
	enabled: boolean;
	inputSchema: Record<string, unknown>;
}

// ── External MCP server ───────────────────────────────────────────────────────
export interface ExternalMcpServer {
	id: string;
	name: string;
	command: string;
	args: string[];
	env: Record<string, string>;
	enabled: boolean;
}

export interface ExternalMcpToolInfo {
	name: string;
	displayName: string;
	description: string;
	enabled: boolean;
	inputSchema: Record<string, unknown>;
	serverId: string;
	serverName: string;
}

// ── App settings ──────────────────────────────────────────────────────────────
export interface AppSettings {
	ai: { provider: string; apiKey: string; model: string; baseUrl: string; timeoutSecs: number };
	nixiumOptions: Record<EditorExtensionKey, boolean>;
	extensions: Record<string, boolean>;
}

export const DEFAULT_NIXIUM_OPTIONS: AppSettings['nixiumOptions'] = {
	wordWrap: false,
	lineNumbers: true,
	foldGutter: true,
	autoBrackets: true,
	highlightActiveLine: true,
	autocompletion: true,
};

export const DEFAULT_SETTINGS: AppSettings = {
	ai: { provider: 'openai', apiKey: '', model: 'gpt-4o-mini', baseUrl: '', timeoutSecs: 120 },
	nixiumOptions: { ...DEFAULT_NIXIUM_OPTIONS },
	extensions: {},
};

export const SETTINGS_KEY = 'nixium-settings';

export function loadSettings(): AppSettings {
	try {
		const raw = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') as Partial<AppSettings>;
		return {
			...DEFAULT_SETTINGS, ...raw,
			ai: { ...DEFAULT_SETTINGS.ai, ...(raw.ai ?? {}) },
			nixiumOptions: { ...DEFAULT_NIXIUM_OPTIONS, ...(raw.nixiumOptions ?? {}) },
			extensions: { ...(raw.extensions ?? {}) },
		};
	} catch { return DEFAULT_SETTINGS; }
}

export interface PaletteCommand {
	id: string;
	label: string;
	description?: string;
	keybinding?: string;
	action: () => void;
}

export const EDITOR_OPTION_ITEMS: Array<{ key: EditorExtensionKey; label: string; desc: string }> = [
	{ key: 'wordWrap',            label: 'Word Wrap',             desc: 'Wrap long lines instead of scrolling' },
	{ key: 'lineNumbers',         label: 'Line Numbers',          desc: 'Show line numbers in the gutter' },
	{ key: 'foldGutter',          label: 'Code Folding',          desc: 'Collapsible fold markers in the gutter' },
	{ key: 'autoBrackets',        label: 'Auto Close Brackets',   desc: 'Automatically insert closing brackets and quotes' },
	{ key: 'highlightActiveLine', label: 'Highlight Active Line', desc: 'Highlight the line the cursor is on' },
	{ key: 'autocompletion',      label: 'Autocompletion',        desc: 'Suggest completions while typing (Ctrl+Space)' },
];
