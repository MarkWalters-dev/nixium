import type { EditorExtensionKey } from '$lib/useCodeMirror';

export interface AppSettings {
	ai: { provider: string; apiKey: string; model: string; baseUrl: string };
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
	ai: { provider: 'openai', apiKey: '', model: 'gpt-4o-mini', baseUrl: '' },
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
