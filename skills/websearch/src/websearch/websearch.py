"""Websearch skill implementation."""

from __future__ import annotations

import asyncio
import importlib.util
import json
import os
from pathlib import Path

# httpx is imported inside each `_fetch_*` on first call: importing it at
# module level would put its full import cost on every kernel bootstrap, in
# every session, even when no search is ever run. `find_spec` keeps the
# import-time contract — importing this module still fails with the same
# "No module named 'httpx'" when httpx is absent, so a kernel without httpx
# reports the same unavailable-skill error as before.
if importlib.util.find_spec("httpx") is None:
    raise ModuleNotFoundError("No module named 'httpx'")


def _env_int(name: str, default: int) -> int:
    """Read an int from the environment, falling back to default on bad values."""
    try:
        return int(os.environ[name])
    except (KeyError, ValueError):
        return default


def _agent_dir() -> Path:
    """Resolve the Beehive config dir the same way the runtime does."""
    raw = (
        os.environ.get("BEEHIVE_CODING_AGENT_DIR")
        or os.environ.get("BEEHIVE_CODING_AGENT_DIR")
        or str(Path.home() / ".beehive")
    )
    return Path(raw).expanduser()


# Per provider id (the host passes the /settings choice as BEEHIVE_WEBSEARCH_PROVIDER;
# the id is also the key's auth.json slot): the key's env var, the provider's name (its
# /mcp row is "<name> (web search)"), and where to get a key.
_PROVIDERS = {
    "serper": ("SERPER_API_KEY", "Serper", "https://serper.dev"),
    "eco": ("YOU_API_KEY", "You.com Eco", "https://you.com/platform"),
}


def _resolve_api_key(provider: str) -> str:
    # Read auth.json on each call (not just the injected env var) so a key added
    # via /login after the kernel started is still picked up. Env var wins.
    key_env, _, _ = _PROVIDERS[provider]
    env_key = os.environ.get(key_env, "").strip()
    if env_key:
        return env_key

    try:
        auth = json.loads((_agent_dir() / "auth.json").read_text())
        cred = auth.get(provider) if isinstance(auth, dict) else None
        if isinstance(cred, dict) and cred.get("type") == "api_key":
            return _resolve_config_value(str(cred.get("key") or ""))
    except (OSError, ValueError):
        pass
    return ""


def _resolve_config_value(value: str) -> str:
    # Stored keys may be a literal or an env-var name; "!command" refs can't be run
    # safely here, so skip them (the agent injects those resolved at build time).
    value = value.strip()
    if not value or value.startswith("!"):
        return ""
    return (os.environ.get(value) or value).strip()


def _format_serper_results(data: dict, query: str, num_results: int = 5) -> str:
    """Format a Serper API response into readable text."""
    sections: list[str] = []

    kg = data.get("knowledgeGraph")
    if kg:
        kg_lines: list[str] = []
        title = (kg.get("title") or "").strip()
        if title:
            kg_lines.append(f"Knowledge Graph: {title}")
        description = (kg.get("description") or "").strip()
        if description:
            kg_lines.append(description)
        for key, value in (kg.get("attributes") or {}).items():
            text = str(value).strip()
            if text:
                kg_lines.append(f"{key}: {text}")
        if kg_lines:
            sections.append("\n".join(kg_lines))

    for i, result in enumerate((data.get("organic") or [])[:num_results]):
        title = (result.get("title") or "").strip() or "Untitled"
        lines = [f"Result {i}: {title}"]
        link = (result.get("link") or "").strip()
        if link:
            lines.append(f"URL: {link}")
        snippet = (result.get("snippet") or "").strip()
        if snippet:
            lines.append(snippet)
        sections.append("\n".join(lines))

    people_also_ask = data.get("peopleAlsoAsk") or []
    if people_also_ask:
        max_q = max(1, min(3, len(people_also_ask)))
        questions: list[str] = []
        for item in people_also_ask[:max_q]:
            question = (item.get("question") or "").strip()
            if not question:
                continue
            entry = f"Q: {question}"
            answer = (item.get("snippet") or "").strip()
            if answer:
                entry += f"\nA: {answer}"
            questions.append(entry)
        if questions:
            sections.append("People Also Ask:\n" + "\n".join(questions))

    if not sections:
        return f"No results returned for query: {query}"

    return "\n\n---\n\n".join(sections)


async def _fetch_serper(query: str, api_key: str, timeout: int = 45, num_results: int = 5) -> str:
    """Execute a single Serper API search."""
    import httpx

    try:
        async with httpx.AsyncClient(timeout=timeout) as client:
            resp = await client.post(
                "https://google.serper.dev/search",
                json={"q": query},
                headers={
                    "X-API-KEY": api_key,
                    "Content-Type": "application/json",
                },
            )
            resp.raise_for_status()
            data = resp.json()
    except httpx.HTTPStatusError as e:
        body = e.response.text if e.response is not None else ""
        raise RuntimeError(f"Serper search error ({e.response.status_code}): {body}") from e

    return _format_serper_results(data, query, num_results=num_results)


def _format_eco_results(data: dict, query: str, num_results: int = 5) -> str:
    """Format a You.com Eco Search response like Serper's organic results."""
    organic = [
        {"title": item.get("title"), "link": item.get("url"), "snippet": item.get("description")}
        for item in (data.get("results") or {}).get("web") or []
    ]
    return _format_serper_results({"organic": organic}, query, num_results=num_results)


async def _fetch_eco(query: str, api_key: str, timeout: int = 45, num_results: int = 5) -> str:
    """Execute a single You.com Eco Search (at most 10 results per page)."""
    import httpx

    try:
        async with httpx.AsyncClient(timeout=timeout) as client:
            resp = await client.get(
                "https://ydc-index.io/v1/eco_search",
                params={"query": query},
                headers={"X-API-Key": api_key},
            )
            resp.raise_for_status()
            data = resp.json()
    except httpx.HTTPStatusError as e:
        body = e.response.text if e.response is not None else ""
        raise RuntimeError(f"Eco search error ({e.response.status_code}): {body}") from e

    return _format_eco_results(data, query, num_results=num_results)


async def _fetch_host_search(
    provider: str, query: str, *, timeout: int, num_results: int
) -> str:
    """Search through the Beehive host using its provider login."""
    from rlm import host_request

    name = "Claude" if provider == "claude" else "ChatGPT"
    if type(num_results) is not int or num_results < 1:
        raise ValueError(f"{name} web search requires a positive integer number of results")
    try:
        reply = await asyncio.wait_for(
            host_request(f"{provider}_web_search", {"query": query, "numResults": num_results}),
            timeout=timeout,
        )
    except asyncio.TimeoutError as exc:
        raise RuntimeError(f"{name} web search timed out") from exc
    text = reply.get("text")
    if not isinstance(text, str) or not text.strip():
        raise RuntimeError(f"{name} web search returned no result text")
    return text


def _missing_key(provider: str) -> str:
    _, name, signup_url = _PROVIDERS[provider]
    return (
        f"Web search is not set up yet: no {name} API key is configured.\n"
        "Tell the user how to enable it:\n"
        f"  1. Get a free API key at {signup_url} (sign up, copy the key).\n"
        f"  2. In Beehive, run /mcp, choose \"{name} (web search)\", and paste the key.\n"
        "Do not ask the user to set environment variables. Once the key is saved, web search works automatically."
    )


async def _fetch_keyed(
    provider: str,
    query: str,
    api_key: str,
    *,
    timeout: int,
    num_results: int,
    reserve_fallback: bool,
) -> str:
    fetch = _fetch_eco if provider == "eco" else _fetch_serper
    if not reserve_fallback:
        return await fetch(query, api_key, timeout=timeout, num_results=num_results)
    # The normal cell deadline is 60s. Leave time for the host search even
    # when the primary provider hangs, counting all HTTP phases together.
    primary_timeout = min(timeout, 20)
    try:
        return await asyncio.wait_for(
            fetch(query, api_key, timeout=primary_timeout, num_results=num_results),
            timeout=primary_timeout,
        )
    except asyncio.TimeoutError as exc:
        raise RuntimeError(f"{_PROVIDERS[provider][1]} search timed out") from exc


async def run(
    query: str,
    *,
    max_output: int = 8192,
    timeout: int | None = None,
    num_results: int | None = None,
) -> str:
    """Run a web search with the provider chosen in /settings and return formatted results.

    Args:
        query: Web search query.
        max_output: Truncate output to this many chars.
        timeout: Timeout in seconds for each provider attempt.
        num_results: Requested number of results.

    Returns:
        Formatted search results.
    """
    from rlm import repl

    provider = os.environ.get("BEEHIVE_WEBSEARCH_PROVIDER") or "serper"
    default_timeout = timeout is None and "BEEHIVE_WEBSEARCH_TIMEOUT" not in os.environ
    if timeout is None:
        timeout = _env_int("BEEHIVE_WEBSEARCH_TIMEOUT", 45)
    if num_results is None:
        num_results = _env_int("BEEHIVE_WEBSEARCH_NUM_RESULTS", 5)

    if provider == "chatgpt":
        try:
            result = await _fetch_host_search(provider, query, timeout=timeout, num_results=num_results)
        except Exception as exc:
            result = f"Error searching for '{query}': {exc}"
    else:
        active = repl.is_active()
        reserve_fallback = default_timeout and active
        api_key = "" if provider == "claude" else _resolve_api_key(provider)
        result = None
        primary_error = ""
        try:
            if provider == "claude":
                if not active:
                    raise RuntimeError("Claude web search requires a Beehive kernel")
                result = await _fetch_host_search(
                    provider,
                    query,
                    timeout=min(timeout, 20) if reserve_fallback else timeout,
                    num_results=num_results,
                )
            elif api_key:
                result = await _fetch_keyed(
                    provider,
                    query,
                    api_key,
                    timeout=timeout,
                    num_results=num_results,
                    reserve_fallback=reserve_fallback,
                )
        except Exception as exc:
            primary_error = f"Error searching for '{query}': {exc}"

        # Standalone calls have no host. The bridge itself also rejects a
        # closed connection before sending a request. Neither case probes
        # credentials or makes a network call.
        if result is None and repl.is_active():
            try:
                result = await _fetch_host_search("chatgpt", query, timeout=timeout, num_results=num_results)
            except Exception:
                pass  # Preserve the selected provider's error or setup guidance.
        if result is None:
            if provider != "claude" and not api_key:
                return _missing_key(provider)
            result = primary_error
    output = f'Results for query "{query}":\n\n{result}'

    if len(output) > max_output:
        total = len(output)
        marker = f"\n... [output truncated, {total} chars total] ...\n"
        # Reserve room for the marker so the result stays within max_output.
        half = max(0, (max_output - len(marker)) // 2)
        output = output[:half] + marker + output[len(output) - half:]
        if len(output) > max_output:  # marker alone exceeds the budget
            output = output[:max_output]

    return output
