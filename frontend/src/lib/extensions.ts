/**
 * Extension system types.
 *
 * Extensions are JavaScript ES modules stored on disk at:
 *   ~/.config/nixium/extensions/<name>/
 *
 * Directory layout:
 *   my-extension/
 *     manifest.json   ← required
 *     index.js        ← ES module, required
 *
 * manifest.json schema:
 *   {
 *     "displayName": "My Extension",
 *     "version":     "1.0.0",
 *     "description": "What this extension does.",
 *     "main":        "index.js"   // optional, defaults to index.js
 *   }
 *
 * index.js must export at minimum an `activate` function:
 *   export function activate(api) { ... }
 *   export function deactivate()  { ... }   // optional
 */

/** Parsed manifest.json for an installed extension. */
export interface ExtensionManifest {
	/** Internal ID — matches the extension directory name. */
	name: string;
	/** Human-readable name shown in the Extensions panel. */
	displayName: string;
	version: string;
	description: string;
	/** Entry-point JS filename inside the extension directory. */
	main: string;
}

/** API object passed to extension `activate()` calls. */
export interface ExtensionAPI {
	/** Returns the absolute path of the currently active file, or null. */
	getActiveFilePath(): string | null;

	/** Returns the current text content of the active editor, or null. */
	getActiveFileContent(): string | null;

	/** Opens a file in the nixium. */
	openFile(path: string): Promise<void>;

	/**
	 * Registers a command that appears in the command palette.
	 * The returned function removes the command.
	 */
	registerCommand(
		id: string,
		label: string,
		handler: () => void | Promise<void>
	): () => void;

	/** Shows a brief notification banner for ~3.5 seconds. */
	showNotification(message: string, type?: 'info' | 'error'): void;
}

/** Shape of an extension ES module. */
export interface ExtensionModule {
	activate?(api: ExtensionAPI): void | Promise<void>;
	deactivate?(): void | Promise<void>;
}
