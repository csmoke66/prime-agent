"""Exercise provider selection and fallback without credentials or network calls."""

import asyncio
import os
import unittest
from contextlib import ExitStack
from unittest.mock import AsyncMock, call, patch

import httpx
from rlm import repl
from websearch import websearch


QUERY = "Prime Agent release"
LINKED = "Result 0: Release notes\nURL: https://example.org/releases"
REAL_HOST_REQUEST = repl.host_request
LOGIN_REQUIRED = "ChatGPT web search requires a ChatGPT login"
CLAUDE_LOGIN_REQUIRED = "Claude web search requires an Anthropic OAuth login"


class WebSearchTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        stack = ExitStack()
        self.addCleanup(stack.close)
        self.env = stack.enter_context(patch.dict(os.environ, {}, clear=True))
        self.active = stack.enter_context(patch.object(repl, "is_active", return_value=True))
        self.key = stack.enter_context(
            patch.object(websearch, "_resolve_api_key", return_value="test-key")
        )
        self.serper = stack.enter_context(
            patch.object(websearch, "_fetch_serper", return_value=LINKED)
        )
        self.eco = stack.enter_context(
            patch.object(websearch, "_fetch_eco", return_value=LINKED)
        )
        self.host = stack.enter_context(
            patch.object(
                repl,
                "host_request",
                new=AsyncMock(return_value={"status": "ok", "result": {"text": LINKED}}),
            )
        )

    async def test_direct_chatgpt_uses_only_the_host_and_returns_links(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "chatgpt"
        self.assertIn(LINKED, await websearch.run(QUERY, num_results=3))
        self.host.assert_awaited_once_with({"type": "chatgpt_web_search", "query": QUERY, "numResults": 3})
        self.key.assert_not_called()
        self.serper.assert_not_awaited()
        self.eco.assert_not_awaited()

    async def test_chatgpt_failure_does_not_retry(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "chatgpt"
        self.host.return_value = {"status": "error", "error": LOGIN_REQUIRED}
        self.assertIn(LOGIN_REQUIRED, await websearch.run(QUERY))
        self.host.assert_awaited_once()
        self.key.assert_not_called()

    async def test_chatgpt_rejects_invalid_count_without_sending_a_request(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "chatgpt"
        self.assertIn("positive integer", await websearch.run(QUERY, num_results=0))
        self.host.assert_not_awaited()

    async def test_direct_claude_uses_only_the_host_and_returns_links(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "claude"
        self.assertIn(LINKED, await websearch.run(QUERY, num_results=3))
        self.host.assert_awaited_once_with({"type": "claude_web_search", "query": QUERY, "numResults": 3})
        self.key.assert_not_called()
        self.serper.assert_not_awaited()
        self.eco.assert_not_awaited()

    async def test_claude_error_or_empty_text_falls_back_to_chatgpt_once(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "claude"
        for primary_reply in (
            {"status": "error", "error": CLAUDE_LOGIN_REQUIRED},
            {"status": "ok", "result": {"text": " "}},
        ):
            with self.subTest(primary_reply=primary_reply):
                self.host.reset_mock()
                self.host.side_effect = [primary_reply, {"status": "ok", "result": {"text": LINKED}}]
                self.assertIn(LINKED, await websearch.run(QUERY, num_results=3))
                self.assertEqual(self.host.await_args_list, [
                    call({"type": "claude_web_search", "query": QUERY, "numResults": 3}),
                    call({"type": "chatgpt_web_search", "query": QUERY, "numResults": 3}),
                ])
        self.key.assert_not_called()

    async def test_claude_failure_is_preserved_when_chatgpt_fails(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "claude"
        self.host.side_effect = [
            {"status": "error", "error": CLAUDE_LOGIN_REQUIRED},
            {"status": "error", "error": LOGIN_REQUIRED},
        ]
        result = await websearch.run(QUERY)
        self.assertIn(CLAUDE_LOGIN_REQUIRED, result)
        self.assertNotIn(LOGIN_REQUIRED, result)
        self.assertEqual(self.host.await_args_list, [
            call({"type": "claude_web_search", "query": QUERY, "numResults": 5}),
            call({"type": "chatgpt_web_search", "query": QUERY, "numResults": 5}),
        ])
        self.key.assert_not_called()

    async def test_standalone_claude_does_not_call_host_or_resolve_api_keys(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "claude"
        self.active.return_value = False
        self.assertIn("requires a Prime Agent kernel", await websearch.run(QUERY))
        self.host.assert_not_awaited()
        self.key.assert_not_called()

    async def test_claude_timeout_cancels_host_wait_and_falls_back_once(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "claude"
        stopped = asyncio.Event()

        async def host(request):
            if request["type"] == "claude_web_search":
                try:
                    await asyncio.Future()
                finally:
                    stopped.set()
            return {"status": "ok", "result": {"text": LINKED}}

        self.host.side_effect = host
        self.assertIn(LINKED, await websearch.run(QUERY, timeout=0.05))
        self.assertTrue(stopped.is_set())
        self.assertEqual(self.host.await_args_list, [
            call({"type": "claude_web_search", "query": QUERY, "numResults": 5}),
            call({"type": "chatgpt_web_search", "query": QUERY, "numResults": 5}),
        ])

    async def test_keyed_success_does_not_call_host_and_reserves_time_by_default(self):
        self.assertIn(LINKED, await websearch.run(QUERY))
        self.serper.assert_awaited_once_with(QUERY, "test-key", timeout=20, num_results=5)
        self.host.assert_not_awaited()

    async def test_missing_key_falls_back_for_both_keyed_providers(self):
        self.key.return_value = ""
        for provider in ("serper", "eco"):
            with self.subTest(provider=provider):
                self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = provider
                self.host.reset_mock()
                self.assertIn(LINKED, await websearch.run(QUERY))
                self.host.assert_awaited_once()
        self.serper.assert_not_awaited()
        self.eco.assert_not_awaited()

    async def test_http_error_and_timeout_fall_back_and_keep_explicit_timeout(self):
        self.serper.side_effect = RuntimeError("Serper search error (503): unavailable")
        self.assertIn(LINKED, await websearch.run(QUERY, timeout=30))
        self.serper.assert_awaited_once_with(QUERY, "test-key", timeout=30, num_results=5)
        self.host.assert_awaited_once()

        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "eco"
        self.env["PRIME_AGENT_WEBSEARCH_TIMEOUT"] = "29"
        self.eco.side_effect = httpx.ReadTimeout("Eco request timed out")
        self.host.reset_mock()
        self.assertIn(LINKED, await websearch.run(QUERY))
        self.eco.assert_awaited_once_with(QUERY, "test-key", timeout=29, num_results=5)
        self.host.assert_awaited_once()

    async def test_missing_chatgpt_login_preserves_primary_failure(self):
        self.host.return_value = {"status": "error", "error": LOGIN_REQUIRED}
        self.serper.side_effect = RuntimeError("Serper search error (401): expired key")
        result = await websearch.run(QUERY)
        self.assertIn("Serper search error (401): expired key", result)
        self.assertNotIn(LOGIN_REQUIRED, result)

        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "eco"
        self.key.return_value = ""
        result = await websearch.run(QUERY)
        self.assertIn("no You.com Eco API key is configured", result)
        self.assertIn("https://you.com/platform", result)
        self.assertIn("/mcp", result)
        self.assertNotIn(LOGIN_REQUIRED, result)

    async def test_standalone_calls_skip_host_and_keep_original_timeout(self):
        self.active.return_value = False
        self.serper.side_effect = RuntimeError("Serper search error (503): unavailable")
        self.assertIn("Serper search error (503): unavailable", await websearch.run(QUERY))
        self.serper.assert_awaited_once_with(QUERY, "test-key", timeout=45, num_results=5)
        self.key.return_value = ""
        self.assertIn("no Serper API key is configured", await websearch.run(QUERY))
        self.host.assert_not_awaited()

    async def test_disconnected_bridge_preserves_setup_without_sending(self):
        self.key.return_value = ""
        with (
            patch.object(repl, "host_request", REAL_HOST_REQUEST),
            patch.object(repl, "_loop", asyncio.get_running_loop()),
            patch.object(repl, "_host_closed", True),
            patch.object(repl, "_send") as send,
        ):
            self.assertIn("no Serper API key is configured", await websearch.run(QUERY))
        send.assert_not_called()

    async def test_fallback_output_is_truncated(self):
        self.key.return_value = ""
        self.host.return_value = {"status": "ok", "result": {"text": LINKED + "\n" + "x" * 200}}
        result = await websearch.run(QUERY, max_output=100)
        self.assertLessEqual(len(result), 100)
        self.assertIn("[output truncated,", result)

    async def test_chatgpt_timeout_cancels_host_wait(self):
        self.env["PRIME_AGENT_WEBSEARCH_PROVIDER"] = "chatgpt"
        stopped = asyncio.Event()

        async def stalled_host(_request):
            try:
                await asyncio.Future()
            finally:
                stopped.set()

        self.host.side_effect = stalled_host
        self.assertIn("ChatGPT web search timed out", await websearch.run(QUERY, timeout=0.05))
        self.assertTrue(stopped.is_set())
        self.host.assert_awaited_once()

    async def test_cancelling_primary_does_not_call_the_fallback(self):
        started = asyncio.Event()

        async def stalled_primary(*_args, **_kwargs):
            started.set()
            await asyncio.Future()

        self.serper.side_effect = stalled_primary
        task = asyncio.create_task(websearch.run(QUERY))
        await started.wait()
        task.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await task
        self.host.assert_not_awaited()
