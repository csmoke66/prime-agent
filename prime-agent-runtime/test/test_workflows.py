from __future__ import annotations

import asyncio
import importlib
import unittest
from unittest.mock import patch


rlm_module = importlib.import_module("rlm")


class WorkflowRunTest(unittest.TestCase):
    def test_remote_run_names_the_target_session(self) -> None:
        requests: list[tuple[str, dict]] = []

        async def host(request_type: str, payload: dict) -> dict:
            requests.append((request_type, payload))
            return {"output": {"started": True}}

        with patch.object(rlm_module, "host_request", host):
            result = asyncio.run(
                rlm_module.rlm.workflows.run(
                    "railguard-memory-repair",
                    {"scope": "local"},
                    session="target-session-id",
                )
            )

        self.assertEqual(result, {"started": True})
        self.assertEqual(
            requests,
            [
                (
                    "rlm.workflows.run",
                    {
                        "name": "railguard-memory-repair",
                        "input": {"scope": "local"},
                        "targetSession": "target-session-id",
                    },
                )
            ],
        )

    def test_local_run_does_not_add_a_target(self) -> None:
        requests: list[tuple[str, dict]] = []

        async def host(request_type: str, payload: dict) -> dict:
            requests.append((request_type, payload))
            return {"output": {"summary": "done"}}

        with patch.object(rlm_module, "host_request", host):
            result = asyncio.run(rlm_module.rlm.workflows.run("review", {"topic": "x"}))

        self.assertEqual(result, {"summary": "done"})
        self.assertNotIn("targetSession", requests[0][1])


if __name__ == "__main__":
    unittest.main()
