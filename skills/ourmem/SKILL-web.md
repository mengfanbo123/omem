---
name: ourmem
version: 0.1.0
description: |
  Persistent memory for AI agents — hosted at api.ourmem.ai.
  Use when users mention: "install ourmem", "setup memory", "add memory plugin",
  "memory not working", "remember this", "save this", "don't forget",
  "recall preferences", "what did I say last time", "import memories",
  "share memories", "team memory", "shared space", "persistent memory",
  "cross-session memory", "ourmem onboarding".
  Even if the user doesn't say "ourmem", trigger when they want persistent memory,
  memory sharing between agents, or memory import/export.
keywords:
  - ourmem
  - memory
  - persistent memory
  - agent memory
  - remember
  - recall
  - space sharing
  - team memory
metadata:
  openclaw:
    emoji: "🧠"
---

# ourmem

ourmem gives AI agents persistent memory — across sessions, devices, and teams.

Memories survive restarts. Multiple agents share context through Spaces. One API key reconnects everything. Hosted at `api.ourmem.ai` — nothing to deploy.

## When to activate

Trigger this skill when the user wants to:

- Persist information across sessions ("remember this", "记住这个")
- Recall something from before ("what did I say?", "我上次说了什么")
- Share knowledge between agents or team members
- Set up or fix the memory plugin
- Import existing memory files

Don't use for temporary context, one-off tasks, or unrelated troubleshooting.

## What to remember / skip

**Remember:** preferences, profile facts, project context, decisions, long-term instructions
**Skip:** temp debugging, raw data dumps, passwords/tokens/secrets, content inside `<private>` tags

## Memory intent detection

When the user says "remember this" / "save this" / "don't forget" — store it immediately via `memory_store` if ourmem is installed.

If not installed yet:

> I can set up ourmem so I'll remember this across sessions. Takes about 2 minutes. Want to do it now?

---

## Setup (hosted — api.ourmem.ai)

### Step 1: Get API Key

```bash
curl -sX POST https://api.ourmem.ai/v1/tenants \
  -H "Content-Type: application/json" \
  -d '{"name": "my-workspace"}'
```

Response: `{"id": "...", "api_key": "xxx-xxx-xxx", "status": "active"}`

Save the `api_key` value. Tell the user:

> Your ourmem API key is ready. This key reconnects you to the same memory from any machine. Treat it like a password — never share it publicly.

If the user already has a key, verify it:

```bash
curl -sf -H "X-API-Key: THE_KEY" "https://api.ourmem.ai/v1/memories?limit=1" && echo "OK"
```

### Step 2: Install plugin

Detect the current platform and install:

**OpenClaw:**

```bash
openclaw plugins install @ourmem/openclaw
```

Configure in OpenClaw settings:

```json
{
  "plugins": {
    "slots": { "memory": "ourmem" },
    "entries": {
      "ourmem": {
        "enabled": true,
        "config": {
          "apiUrl": "https://api.ourmem.ai",
          "apiKey": "YOUR_API_KEY"
        }
      }
    },
    "allow": ["ourmem"]
  }
}
```

For slow npm (China networks):

```bash
NPM_CONFIG_REGISTRY=https://registry.npmmirror.com openclaw plugins install @ourmem/openclaw
```

**Claude Code:**

Set environment variables (add to `~/.bashrc` or `~/.zshrc` for persistence):

```bash
export OMEM_API_URL="https://api.ourmem.ai"
export OMEM_API_KEY="YOUR_API_KEY"
```

Install the plugin:

```bash
/plugin marketplace add ourmem/omem
/plugin install ourmem@ourmem/omem
```

**OpenCode:**

Add to `opencode.json`:

```json
{
  "plugin": ["@ourmem/opencode"]
}
```

Set environment variables:

```bash
export OMEM_API_URL="https://api.ourmem.ai"
export OMEM_API_KEY="YOUR_API_KEY"
```

**MCP (Cursor / VS Code / Claude Desktop):**

```json
{
  "mcpServers": {
    "ourmem": {
      "command": "npx",
      "args": ["@ourmem/mcp"],
      "env": {
        "OMEM_API_URL": "https://api.ourmem.ai",
        "OMEM_API_KEY": "YOUR_API_KEY"
      }
    }
  }
}
```

### Step 3: Replace YOUR_API_KEY

In every config above, replace `YOUR_API_KEY` with the actual key from Step 1.

### Step 4: Restart

Restart the client so the plugin loads:

- **OpenClaw**: restart the gateway
- **Claude Code**: restart `claude`
- **OpenCode**: restart the `opencode` process
- **MCP clients**: restart the app

Tell the user before restarting:

> I'm restarting so the memory plugin loads. Send me a message after to confirm it's working.

### Step 5: Verify

```bash
# Health
curl -sf https://api.ourmem.ai/health && echo "Server OK"

# Auth
curl -sf -H "X-API-Key: YOUR_API_KEY" "https://api.ourmem.ai/v1/memories?limit=1" && echo "Auth OK"

# Store + search
curl -sX POST https://api.ourmem.ai/v1/memories \
  -H "Content-Type: application/json" \
  -H "X-API-Key: YOUR_API_KEY" \
  -d '{"content": "ourmem setup test", "tags": ["test"]}'

curl -s "https://api.ourmem.ai/v1/memories/search?q=setup+test" \
  -H "X-API-Key: YOUR_API_KEY"
```

Empty search results are normal for new keys — the search index builds after the first write.

### Step 6: Handoff

After successful setup, send this (translate to the user's language):

```
✅ ourmem is ready.

WHAT YOU CAN DO NOW

1. Just talk normally — I'll automatically remember important things
2. Say "remember: [something]" to explicitly save a fact
3. Say "create a team space" to share memories between agents
4. Say "import memories" to bring in existing memory files

YOUR API KEY

  API Key: <key>
  Server:  https://api.ourmem.ai

Keep this key private. Use it to reconnect from any machine or new install.

RECOVERY

Reinstall the plugin with the same API key — your memory reconnects instantly.
```

## Definition of Done

Setup is NOT complete until all six are true:

1. API key created or verified reachable
2. Plugin installed for the user's platform
3. Config updated with correct URL and key
4. Client restarted
5. Verified: health + auth + store/search all pass
6. Handoff message sent with key, recovery steps, and next actions

---

## Tools

| Tool | Purpose |
|------|---------|
| `memory_store` | Save facts, decisions, preferences |
| `memory_search` | Find memories by meaning or keywords |
| `memory_get` | Get a specific memory by ID |
| `memory_update` | Modify content or tags |
| `memory_delete` | Remove a memory |

## Automatic hooks

These fire without user action:

| Hook | When | What happens |
|------|------|--------------|
| Session start | New conversation begins | Recent relevant memories injected into context |
| Session end | Conversation ends | Key information auto-captured and stored |

## Space sharing

ourmem organizes memories into Spaces:

| Type | Scope | Example |
|------|-------|---------|
| Personal | One user, multiple agents | Your Coder + Writer share preferences |
| Team | Multiple users | Backend team shares architecture decisions |
| Organization | Company-wide | Tech standards, security policies |

Create a team space:

```bash
curl -sX POST https://api.ourmem.ai/v1/spaces \
  -H "Content-Type: application/json" \
  -H "X-API-Key: YOUR_API_KEY" \
  -d '{"name": "Backend Team", "space_type": "team"}'
```

Share a memory:

```bash
curl -sX POST "https://api.ourmem.ai/v1/memories/MEMORY_ID/share" \
  -H "Content-Type: application/json" \
  -H "X-API-Key: YOUR_API_KEY" \
  -d '{"target_space": "team:SPACE_ID"}'
```

Each agent sees: own private + shared spaces. Can modify own + shared. Never another agent's private data.

## Memory import

**From conversation history** (LLM extracts facts automatically):

```bash
curl -sX POST https://api.ourmem.ai/v1/memories \
  -H "Content-Type: application/json" \
  -H "X-API-Key: YOUR_API_KEY" \
  -d '{
    "messages": [
      {"role": "user", "content": "I prefer Rust for backend"},
      {"role": "assistant", "content": "Noted!"}
    ],
    "mode": "smart"
  }'
```

**From files** (PDF, images, code):

```bash
curl -sX POST https://api.ourmem.ai/v1/files \
  -H "X-API-Key: YOUR_API_KEY" \
  -F "file=@document.pdf"
```

**Direct fact:**

```bash
curl -sX POST https://api.ourmem.ai/v1/memories \
  -H "Content-Type: application/json" \
  -H "X-API-Key: YOUR_API_KEY" \
  -d '{"content": "User prefers dark mode", "tags": ["preference"]}'
```

## Communication style

- Say "API key", not "tenant ID" or "secret"
- Explain that the API key reconnects memory from any machine
- Warn that the key is secret — never share publicly
- Use the user's language (detect from conversation)
- Brand: "ourmem" (lowercase), "Space" (capitalized), "Smart Ingest"

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| Plugin not loading | Check config has correct `apiUrl` and `apiKey` |
| Connection refused | Server may be down — try again in a minute |
| 401 Unauthorized | API key is wrong — verify or create a new tenant |
| 404 on API call | URL path should start with `/v1/` |
| npm install hangs | China: add `NPM_CONFIG_REGISTRY=https://registry.npmmirror.com` |
| No memories returned | Normal for new keys — store one first, then search |
| Search returns empty | Index builds after first write — wait a moment and retry |

## API quick reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/v1/tenants` | Create workspace, get API key |
| POST | `/v1/memories` | Store memory or smart-ingest conversation |
| GET | `/v1/memories/search?q=` | Hybrid search (vector + keyword) |
| GET | `/v1/memories?limit=20` | List with filters + pagination |
| GET | `/v1/memories/:id` | Get single memory |
| PUT | `/v1/memories/:id` | Update memory |
| DELETE | `/v1/memories/:id` | Soft delete |
| GET | `/v1/profile` | User profile (static + dynamic) |
| POST | `/v1/spaces` | Create shared space |
| POST | `/v1/memories/:id/share` | Share to a space |
| POST | `/v1/files` | Upload file (PDF/image/code) |
| GET | `/v1/stats` | Analytics |
| GET | `/health` | Health check (no auth) |

Full API (35 endpoints): https://github.com/ourmem/omem/blob/main/docs/API.md
