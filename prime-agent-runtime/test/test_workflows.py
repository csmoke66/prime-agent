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
                    interrupt=True,
                    continue_turn=True,
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
                        "interrupt": True,
                        "continueTurn": True,
                        "wait": True,
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
        self.assertEqual(requests[0][1]["interrupt"], False)
        self.assertEqual(requests[0][1]["continueTurn"], False)
        self.assertEqual(requests[0][1]["wait"], True)

    def test_status_queries_the_target_run(self) -> None:
        requests: list[tuple[str, dict]] = []

        async def host(request_type: str, payload: dict) -> dict:
            requests.append((request_type, payload))
            return {"runId": "run-1", "state": "running"}

        with patch.object(rlm_module, "host_request", host):
            result = asyncio.run(
                rlm_module.rlm.workflows.status("run-1", session="target-session-id")
            )

        self.assertEqual(result["state"], "running")
        self.assertEqual(
            requests,
            [
                (
                    "rlm.workflows.status",
                    {"runId": "run-1", "targetSession": "target-session-id"},
                )
            ],
        )


if __name__ == "__main__":
    unittest.main()
