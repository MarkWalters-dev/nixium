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
	ViewPlugin,
	type ViewUpdate,
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
	| 'autocompletion'
	| 'minimap';

// ---------------------------------------------------------------------------
// Minimap ViewPlugin
// ---------------------------------------------------------------------------
const MINIMAP_W = 72;

function createMinimapPlugin() {
	return ViewPlugin.fromClass(
		class {
			container: HTMLDivElement;
			canvas: HTMLCanvasElement;      // text layer (redrawn on doc change)
			overlay: HTMLCanvasElement;     // viewport box layer (redrawn on scroll)
			raf: number | null = null;
			rafScroll: number | null = null;
			dragging = false;
			onMouseMove: ((e: MouseEvent) => void) | null = null;
			onMouseUp: ((e: MouseEvent) => void) | null = null;
			onScroll: (() => void) | null = null;

			constructor(private view: EditorView) {
				this.container = document.createElement('div');
				Object.assign(this.container.style, {
					position: 'absolute', right: '0', top: '0', bottom: '0',
					width: MINIMAP_W + 'px', zIndex: '8', overflow: 'hidden',
					background: '#13131c',
					borderLeft: '1px solid rgba(255,255,255,0.07)',
					cursor: 'ns-resize',
					// Prevent the browser from hijacking touch gestures on mobile
					touchAction: 'none',
					userSelect: 'none',
				});
				this.container.title = 'Minimap – drag or click to scroll';

				// Text layer sits below, viewport overlay sits on top
				const sharedStyle = 'position:absolute;top:0;left:0;width:100%;height:100%;display:block;';
				this.canvas = document.createElement('canvas');
				this.canvas.style.cssText = sharedStyle;
				this.overlay = document.createElement('canvas');
				this.overlay.style.cssText = sharedStyle + 'pointer-events:none;';
				this.container.appendChild(this.canvas);
				this.container.appendChild(this.overlay);

				// ── Shared scroll helper ─────────────────────────────────────
				const scrollTo = (clientY: number) => {
					const rect = this.container.getBoundingClientRect();
					const frac = Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
					const sd = this.view.scrollDOM;
					sd.scrollTop = frac * (sd.scrollHeight - sd.clientHeight);
				};

				// ── Mouse drag ──────────────────────────────────────────────
				this.container.addEventListener('mousedown', (e) => {
					e.preventDefault();
					this.dragging = true;
					scrollTo(e.clientY);

					this.onMouseMove = (ev: MouseEvent) => { if (this.dragging) scrollTo(ev.clientY); };
					this.onMouseUp   = () => { this.dragging = false; };
					document.addEventListener('mousemove', this.onMouseMove);
					document.addEventListener('mouseup',   this.onMouseUp, { once: true });
				});

				// ── Touch drag ──────────────────────────────────────────────
				this.container.addEventListener('touchstart', (e) => {
					e.preventDefault();
					this.dragging = true;
					scrollTo(e.touches[0].clientY);
				}, { passive: false });

				this.container.addEventListener('touchmove', (e) => {
					e.preventDefault();
					if (this.dragging) scrollTo(e.touches[0].clientY);
				}, { passive: false });

				this.container.addEventListener('touchend', () => { this.dragging = false; });

				view.dom.appendChild(this.container);
				// Indent scroller content so it isn't hidden behind the minimap
				view.scrollDOM.style.paddingRight = MINIMAP_W + 'px';
				// Redraw viewport box on every scroll tick (RAF-debounced)
				this.onScroll = () => this.scheduleOverlay(view);
				view.scrollDOM.addEventListener('scroll', this.onScroll, { passive: true });
				this.schedule(view);
			}

			update(update: ViewUpdate) {
				if (update.docChanged || update.viewportChanged || update.geometryChanged) {
					this.schedule(update.view);
				}
			}

			// Schedule a full redraw (text + overlay) – for doc/geometry changes
			schedule(view: EditorView) {
				if (this.raf !== null) return;
				this.raf = requestAnimationFrame(() => {
					this.raf = null;
					this.drawText(view);
					this.drawOverlay(view);
				});
			}

			// Schedule only the overlay redraw – for scroll events
			scheduleOverlay(view: EditorView) {
				if (this.rafScroll !== null) return;
				this.rafScroll = requestAnimationFrame(() => {
					this.rafScroll = null;
					this.drawOverlay(view);
				});
			}

			// Draw text lines onto the background canvas
			drawText(view: EditorView) {
				const W = this.container.clientWidth || MINIMAP_W;
				const H = this.container.clientHeight || 400;
				this.canvas.width = W;
				this.canvas.height = H;
				const ctx = this.canvas.getContext('2d');
				if (!ctx) return;

				ctx.fillStyle = '#13131c';
				ctx.fillRect(0, 0, W, H);

				const doc = view.state.doc;
				const totalLines = doc.lines;
				const lineH = H / totalLines;

				for (let i = 1; i <= totalLines; i++) {
					const line = doc.line(i);
					const trimmed = line.text.trimStart();
					const indent = (line.length - trimmed.length) * 0.4;
					const w = Math.min(W - 4, trimmed.length * 0.5);
					const y = ((i - 1) / totalLines) * H;
					if (w > 0) {
						ctx.fillStyle = 'rgba(145, 185, 255, 0.22)';
						ctx.fillRect(2 + indent, y + 0.5, w, Math.max(0.5, lineH - 0.75));
					}
				}
			}

			// Draw the viewport box onto the transparent overlay canvas
			drawOverlay(view: EditorView) {
				const W = this.container.clientWidth || MINIMAP_W;
				const H = this.container.clientHeight || 400;
				this.overlay.width = W;
				this.overlay.height = H;
				const ctx = this.overlay.getContext('2d');
				if (!ctx) return;

				ctx.clearRect(0, 0, W, H);

				const sd = view.scrollDOM;
				const scrollH = sd.scrollHeight;
				const clientH = sd.clientHeight;
				const ratio = scrollH > 0 ? H / scrollH : 1;
				const vpH = Math.max(16, clientH * ratio);
				const vpY = sd.scrollTop * ratio;
				ctx.fillStyle = 'rgba(255,255,255,0.06)';
				ctx.fillRect(0, vpY, W, vpH);
				ctx.strokeStyle = 'rgba(255,255,255,0.18)';
				ctx.lineWidth = 1;
				ctx.beginPath();
				ctx.rect(0.5, vpY + 0.5, W - 1, vpH - 1);
				ctx.stroke();
			}

			destroy() {
				if (this.raf !== null) cancelAnimationFrame(this.raf);
				if (this.rafScroll !== null) cancelAnimationFrame(this.rafScroll);
				if (this.onMouseMove) document.removeEventListener('mousemove', this.onMouseMove);
				if (this.onMouseUp)   document.removeEventListener('mouseup',   this.onMouseUp);
				if (this.onScroll)    this.view.scrollDOM.removeEventListener('scroll', this.onScroll);
				this.view.scrollDOM.style.paddingRight = '';
				this.container.remove();
			}
		}
	);
}

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
	const minimapCompartment          = new Compartment();

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

			// Minimap (defaults off)
			minimapCompartment.of([]),

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
			case 'minimap':
				effect = minimapCompartment.reconfigure(enabled ? createMinimapPlugin() : []);
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
