/**
 * Svelte action that fires a callback when the user clicks outside the node.
 *
 * Usage:
 *   <div use:clickOutside={() => (open = false)}>…</div>
 */
export function clickOutside(node: HTMLElement, callback: () => void) {
	function handler(e: MouseEvent) {
		if (!node.contains(e.target as Node)) callback();
	}
	document.addEventListener('mousedown', handler, true);
	return {
		destroy() { document.removeEventListener('mousedown', handler, true); },
	};
}
