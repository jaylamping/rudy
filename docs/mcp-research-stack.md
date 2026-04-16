# MCP stack for easy research (Cursor)

This is the **minimal two-server setup** recommended for Murphy: one for **web / docs / products**, one for **papers and citations**. Both run via `npx` so you do not maintain local clones.

## 1. Tavily (web search, extract, map, crawl)

**Get a key:** [app.tavily.com](https://app.tavily.com/home) (free tier available).

**Cursor:** Settings → MCP → edit user `mcp.json` (or use project MCP if you prefer per-repo keys).

```json
"tavily": {
  "command": "npx",
  "args": ["-y", "tavily-mcp@latest"],
  "env": {
    "TAVILY_API_KEY": "YOUR_KEY_HERE"
  }
}
```

Optional defaults (fewer arguments each call), e.g. more results:

```json
"env": {
  "TAVILY_API_KEY": "YOUR_KEY_HERE",
  "DEFAULT_PARAMETERS": "{\"max_results\": 15, \"search_depth\": \"advanced\"}"
}
```

**Official remote (OAuth or key in URL):** see [tavily-ai/tavily-mcp README](https://github.com/tavily-ai/tavily-mcp) — e.g. `npx -y mcp-remote https://mcp.tavily.com/mcp/?tavilyApiKey=...` if you prefer no local `tavily-mcp` package.

## 2. Semantic Scholar (papers, authors, citations, recommendations)

**Package:** [`@xbghc/semanticscholar-mcp`](https://www.npmjs.com/package/@xbghc/semanticscholar-mcp) (tools like `search_papers`, `get_paper`, `get_paper_citations`, `get_recommendations`, etc.).

**API key (optional but recommended):** request at [Semantic Scholar API](https://www.semanticscholar.org/product/api) for higher rate limits. The server works **without** a key with stricter limits.

```json
"semantic-scholar": {
  "command": "npx",
  "args": ["-y", "@xbghc/semanticscholar-mcp"],
  "env": {
    "SEMANTIC_SCHOLAR_API_KEY": ""
  }
}
```

Set `SEMANTIC_SCHOLAR_API_KEY` to your key string when you have one; leave empty to use the anonymous quota.

## 3. After editing `mcp.json`

1. Save the file.
2. In Cursor MCP settings, **refresh** / toggle the servers off and on once.
3. In chat or Agent, ask explicitly when needed, e.g. “Use Tavily to find …” or “Search Semantic Scholar for …” until you are used to which tool answers which question.

## 4. Security

- Prefer **environment variables** or Cursor’s secret fields over committing keys to git.
- **Rotate any API token** that has ever appeared in a chat log, screenshot, or shared config file.

## 5. Optional third server (later)

- **Firecrawl** — best when you must **render JS-heavy pages** or crawl a whole docs site; heavier than Tavily for simple “find the official doc” tasks.
- **Brave Search MCP** — another general web search option if you already pay for Brave Search API.

## 6. More discovery

- Curated lists: [awesome-mcp-servers](https://github.com/wong2/awesome-mcp-servers), [mcpservers.org](https://mcpservers.org/).
- Multi-source papers + PDFs: [openags/paper-search-mcp](https://github.com/openags/paper-search-mcp) (more setup than the npm Semantic Scholar server above).
