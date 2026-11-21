---
name: websearch
description: Search the web with the provider chosen in /settings (Serper, You.com Eco, or ChatGPT). Returns source-linked results and uses ChatGPT as a fallback when available.
---

# Web Search

Search the web with the provider chosen in `/settings` → **Agents** → **Web search provider**:
Serper (Google results, the default), You.com Eco Search, or ChatGPT.

## Setup

For ChatGPT, sign in to ChatGPT through `/login` in Prime Agent. No API key is needed.
If Serper or You.com Eco has no configured key or its search fails, the skill
tries ChatGPT using that login. If the fallback cannot run or fails, the skill
reports the original provider's error or setup instructions.

To set up Serper or You.com Eco, get a free API key (Serper: https://serper.dev,
You.com Eco: https://you.com/platform), then run `/mcp` and choose
**Serper (web search)** or **You.com Eco (web search)** to paste it.
Prime Agent makes the saved key available to this skill automatically.
If the skill reports a missing key, walk the user through these steps;
don't ask them to set environment variables.

Optional overrides (environment variables):

- `PRIME_AGENT_WEBSEARCH_TIMEOUT` - timeout in seconds per provider attempt (default 45). In a Prime Agent kernel, the default Serper or Eco attempt is capped at 20 seconds to leave time for the fallback.
- `PRIME_AGENT_WEBSEARCH_NUM_RESULTS` - requested number of results (default 5; You.com Eco returns at most 10).

## Usage

Call the prepared `websearch` import directly in the Python kernel:

```python
print(await websearch("latest Prime Agent release"))
```
