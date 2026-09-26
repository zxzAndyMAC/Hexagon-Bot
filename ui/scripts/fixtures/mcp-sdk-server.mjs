// reliability 09: independent official SDK peer, pinned in ui/package-lock.json.
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { z } from 'zod'
const server = new McpServer({ name: 'hexagon-contract-fixture', version: '1.0.0' })
server.registerTool('echo', { inputSchema: { text: z.string() } }, async ({ text }) => ({
  content: [{ type: 'text', text }],
}))
await server.connect(new StdioServerTransport())
