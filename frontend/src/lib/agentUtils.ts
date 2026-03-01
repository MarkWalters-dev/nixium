import type { ChatMessage } from '$lib/Chat.svelte';

export const MAX_AGENT_TURNS = 8;

export const AGENT_TOOLS = [
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

export interface XmlCommand {
	name: string;
	path: string;
	content: string;
	mcpArgs?: string;
	mcpName?: string;
}

/**
 * Build the messages array to send to the AI API.
 * Strips internal tool messages for XML mode.
 */
export function buildApiMessages(msgs: ChatMessage[], xmlMode: boolean): unknown[] {
	if (xmlMode) {
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
	const nativeCallIds = new Set<string>();
	for (const m of msgs) {
		if (m.tool_calls?.length) for (const tc of m.tool_calls) nativeCallIds.add(tc.id);
	}
	return msgs.map((m) => {
		if (m.role === 'tool') {
			if (m.tool_call_id && nativeCallIds.has(m.tool_call_id)) {
				return { role: 'tool', tool_call_id: m.tool_call_id, content: m.content };
			}
			return { role: 'user', content: `Tool result for ${m.tool_name ?? 'tool'}:\n${m.content}` };
		}
		if (m.tool_calls?.length) return { role: 'assistant', content: m.content ?? '', tool_calls: m.tool_calls };
		return { role: m.role as string, content: m.content };
	});
}

/**
 * Parse XML-style commands from model output.
 * Recognises write_file, read_file, list_directory, mcp_call.
 */
export function parseXmlCommands(text: string): XmlCommand[] {
	const cmds: XmlCommand[] = [];
	const writeRe = /<write_file\s+path="([^"]+)"\s*>([\s\S]*?)<\/write_file>/g;
	let m: RegExpExecArray | null;
	while ((m = writeRe.exec(text)) !== null) cmds.push({ name: 'write_file', path: m[1], content: m[2] });
	const readRe = /<read_file\s+path="([^"]+)"\s*\/?>/g;
	while ((m = readRe.exec(text)) !== null) cmds.push({ name: 'read_file', path: m[1], content: '' });
	const listRe = /<list_directory\s+path="([^"]+)"\s*\/?>/g;
	while ((m = listRe.exec(text)) !== null) cmds.push({ name: 'list_directory', path: m[1], content: '' });
	const mcpRe = /<mcp_call\s+name="([^"]+)"\s*>([\s\S]*?)<\/mcp_call>/g;
	while ((m = mcpRe.exec(text)) !== null)
		cmds.push({ name: 'mcp_call', path: '', content: '', mcpArgs: m[2].trim(), mcpName: m[1] });
	return cmds;
}

/** Strip XML command tags from the visible message text. */
export function stripXmlCommands(text: string): string {
	return text
		.replace(/<write_file\s+path="[^"]+"\s*>[\s\S]*?<\/write_file>/g, '')
		.replace(/<read_file\s+path="[^"]+"\s*\/?>/g, '')
		.replace(/<list_directory\s+path="[^"]+"\s*\/?>/g, '')
		.replace(/<mcp_call\s+name="[^"]+"\s*>[\s\S]*?<\/mcp_call>/g, '')
		.trim();
}
