/**
 * useCodeMirror.ts
 *
 * A factory that returns a Svelte `use:action` together with imperative
 * controller methods.  Usage in a component:
 *
 *   const { action, getValue, setValue } = createCodeMirrorAction({ ... });
 *
 *   // In the template:
 *   <div use:action />
 *
 * Deliberately avoids wrapper libraries – only bare CodeMirror 6 packages are
 * used as requested.
 */

import { EditorState, Compartment, type Extension } from '@codemirror/state';
import {
	EditorView,
	keymap,
	lineNumbers,
	highlightActiveLineGutter,
	highlightSpecialChars,
	drawSelection,
	dropCursor,
	rectangularSelection,
	crosshairCursor,
	highlightActiveLine,
} from '@codemirror/view';
import {
	defaultKeymap,
	history,
	historyKeymap,
	indentWithTab,
} from '@codemirror/commands';
import {
	indentOnInput,
	syntaxHighlighting,
	defaultHighlightStyle,
	bracketMatching,
	foldGutter,
	foldKeymap,
} from '@codemirror/language';
import { searchKeymap, highlightSelectionMatches, openSearchPanel } from '@codemirror/search';
import {
	autocompletion,
	closeBrackets,
	closeBracketsKeymap,
	completionKeymap,
} from '@codemirror/autocomplete';
import { oneDark } from '@codemirror/theme-one-dark';

// Language support – loaded on demand via detectLanguage().
import { javascript } from '@codemirror/lang-javascript';
import { css } from '@codemirror/lang-css';
import { html } from '@codemirror/lang-html';
import { python } from '@codemirror/lang-python';
import { rust } from '@codemirror/lang-rust';
import { json } from '@codemirror/lang-json';
import { markdown } from '@codemirror/lang-markdown';
import { StreamLanguage } from '@codemirror/language';
import { shell } from '@codemirror/legacy-modes/mode/shell';
import { nix } from '@replit/codemirror-lang-nix';

// ---------------------------------------------------------------------------
// Language detection
// ---------------------------------------------------------------------------

/** Return the CodeMirror language extension best suited to the given filename. */
export function detectLanguage(filename: string): Extension {
	const ext = filename.split('.').pop()?.toLowerCase() ?? '';
	switch (ext) {
		case 'js':
		case 'mjs':
		case 'cjs':
			return javascript();
		case 'ts':
		case 'mts':
			return javascript({ typescript: true });
		case 'jsx':
			return javascript({ jsx: true });
		case 'tsx':
			return javascript({ typescript: true, jsx: true });
		case 'svelte':
		case 'html':
		case 'htm':
			return html();
		case 'css':
		case 'scss':
		case 'sass':
			return css();
		case 'py':
			return python();
		case 'rs':
			return rust();
		case 'json':
		case 'jsonc':
			return json();
		case 'md':
		case 'mdx':
			return markdown();
		case 'sh':
		case 'bash':
		case 'zsh':
		case 'fish':
		case 'ksh':
			return StreamLanguage.define(shell);
		case 'nix':
			return nix();
		default:
			// Fallback: no language-specific highlighting.
			return [];
	}
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export interface CodeMirrorOptions {
	/** Initial document content. */
	initialValue?: string;
	/** Filename used for language auto-detection. */
	filename?: string;
	/** Whether the editor is read-only. */
	readonly?: boolean;
	/**
	 * Called on every document change with the new full content string.
	 * Throttled at the EditorView dispatch level – this fires immediately.
	 */
	onChange?: (value: string) => void;
}

// ---------------------------------------------------------------------------
// Return type
// ---------------------------------------------------------------------------

export interface CodeMirrorController {
	/**
	 * Svelte `use:action` – attach this to an HTML element that will become
	 * the editor host.
	 */
	action: (node: HTMLElement) => { destroy(): void };
	/** Replace the entire document content programmatically. */
	setValue(value: string): void;
	/** Read the current document content. */
	getValue(): string;
	/** Hot-swap the language extension (e.g. when the user opens a new file). */
	setLanguage(filename: string): void;
	/** Programmatically focus the nixium. */
	focus(): void;
	/** Force the editor to re-measure its layout (use after its container becomes visible). */
	requestMeasure(): void;
	/** Open the built-in CodeMirror search panel (Ctrl+F equivalent). */
	openSearch(): void;
	/** Move the cursor and scroll to a given 1-based line and 0-based column. */
	jumpToLine(line: number, col: number): void;
	/** Toggle a named editor extension on or off at runtime. */
	setEditorExtension(key: EditorExtensionKey, enabled: boolean): void;
}

/** Named toggleable editor features. */
export type EditorExtensionKey =
	| 'wordWrap'
	| 'lineNumbers'
	| 'foldGutter'
	| 'autoBrackets'
	| 'highlightActiveLine'
	| 'autocompletion';

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Create a linked (action ↔ controller) pair.
 *
 * The view is only created when the action is mounted on a DOM element.
 * Calling controller methods before mount is a no-op.
 */
export function createCodeMirrorAction(
	options: CodeMirrorOptions = {}
): CodeMirrorController {
	let view: EditorView | null = null;

	// Compartments let us swap individual extensions without rebuilding state.
	const languageCompartment    = new Compartment();
	const readonlyCompartment    = new Compartment();
	const editableCompartment    = new Compartment();
	// Toggleable feature compartments
	const wordWrapCompartment         = new Compartment();
	const lineNumbersCompartment      = new Compartment();
	const foldGutterCompartment       = new Compartment();
	const autoBracketsCompartment     = new Compartment();
	const activeLineCompartment       = new Compartment();
	const autocompletionCompartment   = new Compartment();

	// ---------------------------------------------------------------------------
	// Base extensions that are always present
	// ---------------------------------------------------------------------------
	function buildExtensions(filename: string, readonly: boolean): Extension[] {
		return [
			// Core editing (always on)
			highlightActiveLineGutter(),
			highlightSpecialChars(),
			history(),
			drawSelection(),
			dropCursor(),
			EditorState.allowMultipleSelections.of(true),
			indentOnInput(),
			syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
			bracketMatching(),
			rectangularSelection(),
			crosshairCursor(),
			highlightSelectionMatches(),

			// Key bindings
			keymap.of([
				...closeBracketsKeymap,
				...defaultKeymap,
				...historyKeymap,
				...foldKeymap,
				...searchKeymap,
				...completionKeymap,
				indentWithTab,
			]),

			// Theme
			oneDark,

			// Swappable compartments
			languageCompartment.of(detectLanguage(filename)),
			readonlyCompartment.of(EditorState.readOnly.of(readonly)),
			editableCompartment.of(EditorView.editable.of(!readonly)),

			// Toggleable feature compartments (defaults: all on)
			lineNumbersCompartment.of(lineNumbers()),
			foldGutterCompartment.of(foldGutter()),
			autoBracketsCompartment.of(closeBrackets()),
			activeLineCompartment.of(highlightActiveLine()),
			autocompletionCompartment.of(autocompletion({ defaultKeymap: true })),
			wordWrapCompartment.of([]),

			// Change listener
			EditorView.updateListener.of((update) => {
				if (update.docChanged && options.onChange) {
					options.onChange(update.state.doc.toString());
				}
			}),

			// Make the view fill its container vertically.
			EditorView.theme({
				'&': { height: '100%', fontSize: '14px' },
				'.cm-scroller': {
					overflow: 'auto',
					fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace",
				},
			}),
		];
	}

	// ---------------------------------------------------------------------------
	// Svelte action
	// ---------------------------------------------------------------------------
	function action(node: HTMLElement): { destroy(): void } {
		const state = EditorState.create({
			doc: options.initialValue ?? '',
			extensions: buildExtensions(
				options.filename ?? '',
				options.readonly ?? false
			),
		});

		view = new EditorView({ state, parent: node });

		return {
			destroy() {
				view?.destroy();
				view = null;
			},
		};
	}

	// ---------------------------------------------------------------------------
	// Controller methods
	// ---------------------------------------------------------------------------
	function setValue(value: string): void {
		if (!view) return;
		view.dispatch({
			changes: {
				from: 0,
				to: view.state.doc.length,
				insert: value,
			},
		});
	}

	function getValue(): string {
		return view?.state.doc.toString() ?? '';
	}

	function setLanguage(filename: string): void {
		if (!view) return;
		view.dispatch({
			effects: languageCompartment.reconfigure(detectLanguage(filename)),
		});
	}

	function focus(): void {
		view?.focus();
	}

	/** Force CodeMirror to re-measure its layout (call after the container becomes visible). */
	function requestMeasure(): void {
		view?.requestMeasure();
	}

	/** Open the built-in search panel. */
	function openSearch(): void {
		if (!view) return;
		openSearchPanel(view);
		view.focus();
	}

	/**
	 * Toggle a named editor extension at runtime.
	 * Safe to call before the view is mounted (no-op).
	 */
	function setEditorExtension(key: EditorExtensionKey, enabled: boolean): void {
		if (!view) return;
		let effect;
		switch (key) {
			case 'wordWrap':
				effect = wordWrapCompartment.reconfigure(enabled ? EditorView.lineWrapping : []);
				break;
			case 'lineNumbers':
				effect = lineNumbersCompartment.reconfigure(enabled ? lineNumbers() : []);
				break;
			case 'foldGutter':
				effect = foldGutterCompartment.reconfigure(enabled ? foldGutter() : []);
				break;
			case 'autoBrackets':
				effect = autoBracketsCompartment.reconfigure(enabled ? closeBrackets() : []);
				break;
			case 'highlightActiveLine':
				effect = activeLineCompartment.reconfigure(enabled ? highlightActiveLine() : []);
				break;
			case 'autocompletion':
				effect = autocompletionCompartment.reconfigure(enabled ? autocompletion({ defaultKeymap: true }) : []);
				break;
			default:
				return;
		}
		view.dispatch({ effects: effect });
	}

	/**
	 * Move the cursor to `line` (1-based) and `col` (0-based byte offset in that
	 * line), then scroll that position into the centre of the viewport.
	 */
	function jumpToLine(line: number, col: number): void {
		if (!view) return;
		const lineInfo = view.state.doc.line(Math.max(1, Math.min(line, view.state.doc.lines)));
		const pos = lineInfo.from + Math.max(0, col);
		view.dispatch({
			selection: { anchor: pos },
			effects: EditorView.scrollIntoView(pos, { y: 'center' }),
		});
		view.focus();
	}

	return { action, setValue, getValue, setLanguage, focus, requestMeasure, openSearch, setEditorExtension, jumpToLine };
}
