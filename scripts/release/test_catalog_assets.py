#!/usr/bin/env python3
"""Test battery for the bundled-catalog build assets (catalog spec §5 gates).

Offline: the fixture mode must produce assets that pass the FULL validation
gates, the packer must hard-fail on missing/invalid assets (negative tests),
and the assets must land in the package
layout the installer expects (assemble + verify against a synthetic repo
tree, including the commit-stamped continuous flow).

Run: python3 scripts/release/test_catalog_assets.py  (or: make catalog-assets-gates)
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

SCRIPTS_DIR = Path(__file__).resolve().parent
REPO = SCRIPTS_DIR.parent.parent
sys.path.insert(0, str(SCRIPTS_DIR))

import bundle_catalog  # noqa: E402  (same release-scripts directory)
from assemble_artifacts import TARGET_ALIASES  # noqa: E402  (same directory)

BUNDLER = SCRIPTS_DIR / "bundle_catalog.py"
ASSEMBLER = SCRIPTS_DIR / "assemble_artifacts.py"
VERIFIER = SCRIPTS_DIR / "verify_release.py"
PACKER = REPO / "scripts" / "package_release.py"
# The synthetic repos have no Cargo workspace for cargo-about to scan.
FAKE_CARGO_ABOUT_DIR = SCRIPTS_DIR / "testdata" / "bin"

HOST_TARGET = "x86_64-unknown-linux-gnu"
# Archives carry the TS platform alias (the name the update flow's channel
# manifest requires): beehive-<version>-<platform>.tar.gz.
HOST_ARCHIVE_PLATFORM = TARGET_ALIASES[HOST_TARGET]


def run_cli(script, args, env_extra=None, cwd=None):
    env = dict(os.environ)
    env.pop("GITHUB_TOKEN", None)
    env["PATH"] = f"{FAKE_CARGO_ABOUT_DIR}{os.pathsep}{env['PATH']}"
    if env_extra:
        env.update(env_extra)
    return subprocess.run(
        [sys.executable, str(script), *args],
        capture_output=True, text=True, env=env, cwd=cwd or REPO, check=False,
    )


class SyntheticRepo:
    """A minimal repo tree the assembler/verifier/packer accept."""

    def __init__(self, root: Path, version: str = "9.9.9", sha: str | None = None):
        self.root = root
        self.version = version
        (root / "skills").mkdir(parents=True)
        (root / "skills" / "skill.md").write_text("# skill\n")
        (root / "LICENSE").write_text("license\n")
        (root / "deny.toml").write_text('[licenses]\nallow = ["MIT"]\n')
        (root / "README.md").write_text("readme\n")
        runtime = root / "runtime"
        runtime.mkdir()
        (runtime / "pyproject.toml").write_text("[project]\nname='rlm'\n")
        (runtime / "src" / "rlm").mkdir(parents=True)
        (runtime / "src" / "rlm" / "repl.py").write_text("# fixture REPL\n")
        # The exe-adjacent kernel runtime tree package_release.py stages.
        kernel = root / "beehive-runtime"
        (kernel / "src" / "rlm").mkdir(parents=True)
        (kernel / "pyproject.toml").write_text("[project]\nname='rlm'\n")
        (kernel / "src" / "rlm" / "repl.py").write_text("# repl\n")
        printed = version if sha is None else f"{version}-continuous.{sha}"
        (runtime / "src" / "repl.py").write_text("# repl\n")
        source = root / "beehive.c"
        source.write_text(f'#include <stdio.h>\nint main(void) {{ puts("{printed}"); return 0; }}\n')
        self.raw_binary = root / "bin" / "cargo-beehive"
        self.raw_binary.parent.mkdir()
        subprocess.run(["gcc", "-g", "-Wl,--build-id", "-o",
                        str(self.raw_binary), str(source)], check=True)
        self.binary = root / "bin" / "beehive"
        self.decoder = root / "bin" / f"beehive-{version}-{HOST_ARCHIVE_PLATFORM}.debug.gz"
        split = run_cli(SCRIPTS_DIR / "split_debug.py", [
            "--binary", str(self.raw_binary), "--shipped", str(self.binary),
            "--out", str(self.binary.parent), "--version", version, "--target", HOST_TARGET])
        if split.returncode:
            raise AssertionError(split.stderr)

    def assemble(self, out_dir, catalog_assets=None, sha=None, target=HOST_TARGET):
        args = [
            "--repo-root", str(self.root), "--version", self.version,
            "--target", target, "--binary", str(self.binary),
            "--decoder", str(self.decoder),
            "--runtime-dir", str(self.root / "runtime"),
            "--out-dir", str(out_dir),
        ]
        if sha:
            args += ["--sha", sha]
        if catalog_assets is not None:
            args += ["--catalog-assets", str(catalog_assets)]
        return run_cli(ASSEMBLER, args)


class ValidationGates(unittest.TestCase):

    def test_fixture_passes_the_full_gates(self):
        """Offline build verifier: --fixture produces a VALIDATED snapshot
        (it clears the transport-tuple and service minimums) without any
        network."""
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "assets"
            result = run_cli(BUNDLER, ["generate", "--fixture", "--out", str(out)])
            self.assertEqual(result.returncode, 0, result.stderr)
            facts = json.loads(result.stdout)
            self.assertGreaterEqual(facts["models"]["transportTuples"],
                                    bundle_catalog.MIN_MODEL_TRANSPORT_TUPLES)
            self.assertGreaterEqual(facts["mcpServices"]["services"],
                                    bundle_catalog.MIN_MCP_SERVICES)
            # The verify command (no small-fixture waiver) agrees.
            verify = run_cli(BUNDLER, ["verify", "--out", str(out)])
            self.assertEqual(verify.returncode, 0, verify.stderr)
            self.assertGreaterEqual(
                json.loads(verify.stdout)["mcpServices"]["services"],
                bundle_catalog.MIN_MCP_SERVICES)

    def test_fixture_shapes_match_the_catalog_contract(self):
        bodies = bundle_catalog.fixture_catalog_bodies()
        models = json.loads(bodies["models.bundled.json"])
        self.assertEqual(set(models), {"schemaVersion", "models"})
        self.assertEqual(models["schemaVersion"], 1)
        seen_tuples = set()
        for model in models["models"]:
            # Strict model schema (spec §1.1): no headers, bounded fields.
            self.assertNotIn("headers", model)
            for field in ("id", "name"):
                self.assertTrue(1 <= len(model[field]) <= 1024)
                self.assertFalse(any(ord(ch) < 0x20 for ch in model[field]))
            self.assertLessEqual(len(model["provider"]), 128)
            self.assertLessEqual(len(model["baseUrl"]), 2048)
            self.assertLessEqual(len(model["input"]), 2)
            self.assertTrue(set(model["input"]) <= {"text", "image"})
            self.assertIn(model["api"], ("anthropic-messages", "openai-completions",
                                        "openai-responses", "bedrock-converse-stream",
                                        "google-generative-ai", "google-vertex",
                                        "mistral-conversations", "openai-codex-responses",
                                        "azure-openai-responses"))
            for cost in model["cost"].values():
                self.assertTrue(0 <= cost <= 1_000_000)
            self.assertLessEqual(model["contextWindow"], 100_000_000)
            self.assertLessEqual(model["maxTokens"], 100_000_000)
            seen_tuples.add((model["provider"], model["api"], model["baseUrl"]))
        self.assertGreaterEqual(len(seen_tuples), 42)

        plugins = json.loads(bodies["mcp-services.bundled.json"])
        # Envelope: exactly version, counts, entries, in order.
        self.assertEqual(list(plugins), ["version", "counts", "entries"])
        self.assertEqual(plugins["version"], 2)
        self.assertEqual(set(plugins["counts"]), set(bundle_catalog.COUNTS_KEYS))
        transports = {"http", "http-template", "sse", "stdio"}
        auths = {"oauth", "api_key", "none", "unknown"}
        readiness = {"oauth-ready", "user-setup", "prime-restricted", "unknown"}
        setups, verifications = set(), set()
        for entry in plugins["entries"]:
            self.assertRegex(entry["server"], r"^[a-z0-9][a-z0-9-]{0,63}$")
            self.assertIn(entry["transport"]["type"], transports)
            self.assertIn(entry["auth"]["strategy"], auths)
            self.assertIn(entry["setup"]["readiness"], readiness)
            setups.add(entry["setup"]["status"])
            verifications.add(entry["verification"]["status"])
            self.assertEqual(entry["aliases"],
                             sorted(a.lower() for a in entry["aliases"]))
            self.assertNotIn(entry["server"], entry["aliases"])
            self.assertNotIn("clientId", json.dumps(entry))
            self.assertNotIn("clientSecret", json.dumps(entry))
        # Every shape branch is exercised.
        self.assertEqual(setups, {"ready", "requires-setup"})
        self.assertEqual(verifications, {"metadata-reviewed", "unverified"})
        self.assertEqual(plugins["counts"]["total"], len(plugins["entries"]))
        self.assertGreaterEqual(len(plugins["entries"]), 68)

    def test_validation_rejects_bad_envelopes_and_minimums(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            good = json.loads(bundle_catalog.fixture_catalog_bodies()
                              ["mcp-services.bundled.json"])
            (out / "models.bundled.json").write_text(
                json.dumps({"schemaVersion": 1, "models": []}))
            (out / "mcp-services.bundled.json").write_text(json.dumps(good))
            with self.assertRaises(SystemExit):
                bundle_catalog.validate_bundled_model_catalog(
                    out / "models.bundled.json")  # 0 tuples < the minimum
            small = {"schemaVersion": 1, "models": [json.loads(
                bundle_catalog.fixture_catalog_bodies()["models.bundled.json"]
            )["models"][0]]}
            (out / "models.bundled.json").write_text(json.dumps(small))
            # The small-fixture waiver skips the minimum-count gate only.
            bundle_catalog.validate_bundled_model_catalog(
                out / "models.bundled.json", allow_small_fixture=True)
            bad_version = dict(good, version=3)
            (out / "mcp-services.bundled.json").write_text(json.dumps(bad_version))
            with self.assertRaises(SystemExit):
                bundle_catalog.validate_bundled_mcp_catalog(
                    out / "mcp-services.bundled.json", allow_small_fixture=True)
            (out / "mcp-services.bundled.json").write_text("{not json")
            with self.assertRaises(SystemExit):
                bundle_catalog.validate_bundled_mcp_catalog(
                    out / "mcp-services.bundled.json", allow_small_fixture=True)
            # A missing asset fails the dir gate with an actionable message.
            with contextlib.redirect_stderr(io.StringIO()) as captured, \
                    self.assertRaises(SystemExit):
                bundle_catalog.validate_bundled_catalog_dir(out / "nowhere")
            self.assertIn("Missing bundled catalog asset", captured.getvalue())

    def test_fixture_covers_paste_flow_and_builtin_shapes(self):
        """The special entries the runtime MCP lane consumes: one shared
        credentialSet alias pair (pasteable), two distinct credential sets
        (never pasteable), an api-key setup field, and the legacy builtins."""
        plugins = json.loads(
            bundle_catalog.fixture_catalog_bodies()["mcp-services.bundled.json"])
        by_server = {entry["server"]: entry for entry in plugins["entries"]}
        pasteable = by_server["fixture-paste-single"]
        sets = {field["credentialSet"] for field in pasteable["setup"]["fields"]}
        self.assertEqual(len(pasteable["setup"]["fields"]), 2)
        self.assertEqual(sets, {"fixture-pat"})  # alias names, one credential
        multi = by_server["fixture-paste-multi-cred"]
        sets = {field["credentialSet"] for field in multi["setup"]["fields"]}
        self.assertEqual(len(sets), 2)  # fail-closed: NOT pasteable
        api_key = by_server["fixture-api-key"]
        self.assertEqual(api_key["setup"]["fields"][0]["kind"], "api-key")
        for builtin in ("linear", "notion"):
            self.assertTrue(by_server[builtin]["legacyBuiltin"])
        # Counts stay derived: total counts every entry including specials.
        self.assertEqual(plugins["counts"]["total"], len(plugins["entries"]))
        self.assertEqual(
            plugins["counts"]["total"],
            bundle_catalog.FIXTURE_MCP_SERVICES + 5)

    def test_small_fixture_verify_waiver(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "assets"
            out.mkdir()
            small_models = {"schemaVersion": 1, "models": [
                {"id": "x", "name": "X", "api": "openai-responses",
                 "provider": "openai", "baseUrl": "https://api.openai.com/v1",
                 "reasoning": False, "input": ["text"],
                 "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0},
                 "contextWindow": 128000, "maxTokens": 4096}]}
            small_plugins = {"version": 2, "counts": {}, "entries": []}
            (out / "models.bundled.json").write_text(json.dumps(small_models))
            (out / "mcp-services.bundled.json").write_text(json.dumps(small_plugins))
            strict = run_cli(BUNDLER, ["verify", "--out", str(out)])
            self.assertNotEqual(strict.returncode, 0)
            self.assertIn("transport tuples", strict.stderr)
            waived = run_cli(BUNDLER,
                             ["verify", "--out", str(out), "--allow-small-fixture"])
            self.assertEqual(waived.returncode, 0, waived.stderr)

    def test_fixture_http_template_entries_match_the_runtime_schema(self):
        """Regression (Cursor Bugbot): the fixture's http-template entries
        must parse under the runtime's strict catalog_schema — template
        variables are {name, description} objects and the entry url is empty
        (a fail-closed runtime parse drops the whole asset otherwise)."""
        plugins = json.loads(bundle_catalog.fixture_catalog_bodies()
                             ["mcp-services.bundled.json"])
        templates = [entry for entry in plugins["entries"]
                     if entry["transport"]["type"] == "http-template"]
        self.assertTrue(templates, "the fixture must exercise the http-template shape")
        for entry in templates:
            transport = entry["transport"]
            self.assertEqual(set(transport), {"type", "template", "variables"})
            self.assertTrue(transport["template"])
            self.assertTrue(transport["variables"])
            for variable in transport["variables"]:
                self.assertEqual(set(variable), {"name", "description"})
                self.assertTrue(variable["name"])
                self.assertTrue(variable["description"])
            self.assertEqual(entry["url"], "")

    def test_validator_rejects_runtime_invalid_plugin_shapes(self):
        """The packaging gate must reject shapes the runtime rejects (the
        pre-fix fixture bug: string template variables + non-empty url)."""
        with tempfile.TemporaryDirectory() as tmp:
            catalog = json.loads(bundle_catalog.fixture_catalog_bodies()
                                 ["mcp-services.bundled.json"])
            entry = next(item for item in catalog["entries"]
                         if item["transport"]["type"] == "http-template")
            entry["transport"]["variables"] = ["region"]
            path = Path(tmp) / "mcp-services.bundled.json"
            path.write_text(json.dumps(catalog))
            with self.assertRaises(SystemExit):
                bundle_catalog.validate_bundled_mcp_catalog(path)

    def test_validator_rejects_models_missing_required_fields(self):
        """The packaging gate must reject model entries the runtime's strict
        parse would drop (missing cost/contextWindow pass the tuple count but
        fail the runtime schema)."""
        with tempfile.TemporaryDirectory() as tmp:
            models = {"schemaVersion": 1, "models": [json.loads(
                bundle_catalog.fixture_catalog_bodies()["models.bundled.json"]
            )["models"][0]]}
            del models["models"][0]["cost"]
            del models["models"][0]["contextWindow"]
            path = Path(tmp) / "models.bundled.json"
            path.write_text(json.dumps(models))
            with self.assertRaises(SystemExit):
                bundle_catalog.validate_bundled_model_catalog(
                    path, allow_small_fixture=True)


class CatalogDirMode(unittest.TestCase):

    def test_catalog_dir_copy_is_byte_identical(self):
        bodies = bundle_catalog.fixture_catalog_bodies()
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            checkout = tmp / "beehive-catalog"
            (checkout / "models").mkdir(parents=True)
            (checkout / "plugins").mkdir()
            (checkout / "models" / "catalog.v1.json").write_text(
                bodies["models.bundled.json"])
            (checkout / "plugins" / "catalog.v2.json").write_text(
                bodies["mcp-services.bundled.json"])
            out = tmp / "assets"
            result = run_cli(
                BUNDLER, ["generate", "--catalog-dir", str(checkout),
                          "--out", str(out)])
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                (out / "models.bundled.json").read_bytes(),
                (checkout / "models" / "catalog.v1.json").read_bytes())
            self.assertEqual(
                (out / "mcp-services.bundled.json").read_bytes(),
                (checkout / "plugins" / "catalog.v2.json").read_bytes())
            # A checkout missing a file fails loudly.
            (checkout / "plugins" / "catalog.v2.json").unlink()
            broken = run_cli(BUNDLER, ["generate", "--catalog-dir", str(checkout),
                                      "--out", str(tmp / "assets2")])
            self.assertNotEqual(broken.returncode, 0)
            self.assertIn("missing", broken.stderr)


class PackerGates(unittest.TestCase):
    """The release packer must hard-fail without validated assets, and valid
    assets must land in the tarball layout the installer expects."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.assets = self.tmp / "catalog-assets"
        result = run_cli(BUNDLER, ["generate", "--fixture", "--out", str(self.assets)])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.repo = SyntheticRepo(self.tmp / "synrepo")

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)
        self._tmp.cleanup()

    def test_packer_fails_without_catalog_assets(self):
        out = self.tmp / "dist-missing"
        result = self.repo.assemble(out, catalog_assets=None)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing bundled catalog assets", result.stderr)
        # Nothing was packed: no archive, no manifest.
        self.assertFalse(
            (out / f"beehive-9.9.9-{HOST_ARCHIVE_PLATFORM}.tar.gz").exists())
        self.assertFalse((out / "manifest.json").exists())

    def test_packers_never_ship_generated_skill_caches(self):
        cache = self.repo.root / "skills" / "__pycache__"
        cache.mkdir()
        (cache / "skill.cpython-311.pyc").write_bytes(b"generated bytecode")
        (self.repo.root / "skills" / "stale.pyc").write_bytes(b"generated bytecode")
        assembled = self.repo.assemble(self.tmp / "assemble-caches", catalog_assets=self.assets)
        self.assertEqual(assembled.returncode, 0, assembled.stderr)
        archive = self.tmp / "assemble-caches" / f"beehive-9.9.9-{HOST_ARCHIVE_PLATFORM}.tar.gz"
        with tarfile.open(archive) as tar:
            self.assertFalse(any("__pycache__" in name or name.endswith(".pyc") for name in tar.getnames()))
        poisoned = self.tmp / "poisoned"
        poisoned.mkdir()
        shutil.copy2(archive, poisoned / archive.name)
        with tarfile.open(poisoned / archive.name) as original:
            items = [(member, original.extractfile(member).read() if member.isfile() else None)
                     for member in original.getmembers()]
        with tarfile.open(poisoned / archive.name, "w:gz") as bad:
            for member, payload in items:
                bad.addfile(member, io.BytesIO(payload) if payload is not None else None)
            extra = tarfile.TarInfo("skills/__pycache__/skill.cpython-311.pyc")
            extra.size = 1
            bad.addfile(extra, io.BytesIO(b"x"))
        checked = run_cli(VERIFIER, ["--dist-dir", str(poisoned), "--version", "9.9.9",
                                     "--target", HOST_TARGET])
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("development cache entry", checked.stderr)
        args = ["--root", str(self.repo.root), "--version", "9.9.9",
                "--binary", str(self.repo.binary),
                "--decoder", str(self.repo.decoder), "--skip-build",
                "--catalog-assets", str(self.assets),
                "--out-dir", str(self.tmp / "package-caches")]
        packaged = run_cli(PACKER, args)
        self.assertEqual(packaged.returncode, 0, packaged.stderr)
        staged = self.tmp / "package-caches" / "beehive-9.9.9-linux-x64" / "skills"
        self.assertFalse(any("__pycache__" in str(path) or path.suffix == ".pyc"
                             for path in staged.rglob("*")))

    def test_packer_fails_on_invalid_assets(self):
        cases = {
            "corrupt-json": ("{\n", "{\n"),
            "wrong-versions": (
                json.dumps({"schemaVersion": 2, "models": []}),
                json.dumps({"version": 3, "entries": []})),
        }
        for label, (models_body, plugins_body) in cases.items():
            bad = self.tmp / f"assets-{label}"
            bad.mkdir()
            (bad / "models.bundled.json").write_text(models_body)
            (bad / "mcp-services.bundled.json").write_text(plugins_body)
            result = self.repo.assemble(self.tmp / f"dist-{label}", catalog_assets=bad)
            self.assertNotEqual(result.returncode, 0, label)
            self.assertIn("Invalid bundled", result.stderr, label)
        # Under-minimum counts fail even with a well-formed envelope.
        small = self.tmp / "assets-small"
        small.mkdir()
        (small / "models.bundled.json").write_text(
            json.dumps({"schemaVersion": 1, "models": []}))
        good_plugins = json.loads(
            bundle_catalog.fixture_catalog_bodies()["mcp-services.bundled.json"])
        (small / "mcp-services.bundled.json").write_text(json.dumps(good_plugins))
        result = self.repo.assemble(self.tmp / "dist-small", catalog_assets=small)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("transport tuples", result.stderr)
        # Missing one of the two files fails too.
        half = self.tmp / "assets-half"
        half.mkdir()
        shutil.copyfile(self.assets / "models.bundled.json",
                        half / "models.bundled.json")
        result = self.repo.assemble(self.tmp / "dist-half", catalog_assets=half)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing bundled catalog asset", result.stderr)

    def test_assets_land_in_the_tarball_the_installer_expects(self):
        out = self.tmp / "dist"
        result = self.repo.assemble(out, catalog_assets=self.assets)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("bundled catalog assets", result.stdout)
        archive = out / f"beehive-9.9.9-{HOST_ARCHIVE_PLATFORM}.tar.gz"
        self.assertTrue(archive.is_file())
        with tarfile.open(archive) as tar:
            names = tar.getnames()
            self.assertIn("models.bundled.json", names)
            self.assertIn("mcp-services.bundled.json", names)
            # The staged assets are byte-identical to the generated ones.
            for name in ("models.bundled.json", "mcp-services.bundled.json"):
                member = tar.extractfile(name)
                self.assertEqual(member.read(),
                                 (self.assets / name).read_bytes(), name)
        # The full end-to-end verifier (payload shape, checksums, manifest,
        # extracted catalog assets, livecheck) passes on the synthetic tree.
        verify = run_cli(VERIFIER, ["--dist-dir", str(out), "--version", "9.9.9",
                                   "--target", HOST_TARGET])
        self.assertEqual(verify.returncode, 0, verify.stderr)
        self.assertIn("catalog assets", verify.stdout)

    def test_continuous_flow_stamps_and_verifies(self):
        # The #274 pipeline: commit-stamped continuous builds carry the assets
        # the same way, and verify enforces the stamp end to end.
        sha = "0123456789abcdef0123456789abcdef01234567"
        repo = SyntheticRepo(self.tmp / "synrepo-continuous",
                             version="9.9.9", sha=sha)
        out = self.tmp / "dist-continuous"
        result = repo.assemble(out, catalog_assets=self.assets, sha=sha)
        self.assertEqual(result.returncode, 0, result.stderr)
        verify = run_cli(VERIFIER, ["--dist-dir", str(out), "--version", "9.9.9",
                                   "--target", HOST_TARGET, "--sha", sha])
        self.assertEqual(verify.returncode, 0, verify.stderr)

    def test_kernel_packer_gates_the_same_assets(self):
        # package_release.py (the exe-adjacent kernel packaging) fails without
        # assets and ships them beside the binary when they are valid.
        args = ["--root", str(self.repo.root), "--version", "9.9.9",
                "--binary", str(self.repo.binary), "--decoder", str(self.repo.decoder),
                "--skip-build", "--out-dir", str(self.tmp / "release-package")]
        without = run_cli(PACKER, args)
        self.assertNotEqual(without.returncode, 0)
        self.assertIn("missing bundled catalog assets", without.stderr)
        with_assets = run_cli(PACKER, [*args, "--catalog-assets", str(self.assets)])
        self.assertEqual(with_assets.returncode, 0, with_assets.stderr)
        staged = self.tmp / "release-package" / "beehive-9.9.9-linux-x64"
        self.assertTrue((staged / "models.bundled.json").is_file())
        self.assertTrue((staged / "mcp-services.bundled.json").is_file())

    def test_kernel_packer_rejects_decoder_leak(self):
        leaked = self.repo.root / "beehive-runtime" / "leak.debug.gz"
        leaked.write_bytes(b"decoder-like payload")
        out = self.tmp / "release-package-leak"
        result = run_cli(PACKER, ["--root", str(self.repo.root), "--version", "9.9.9",
                                  "--binary", str(self.repo.binary),
                                  "--decoder", str(self.repo.decoder), "--out-dir", str(out),
                                  "--catalog-assets", str(self.assets)])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("decoder-like sidecar", result.stderr)
        self.assertFalse(list(out.glob("*.tar.gz")))

    def test_kernel_packer_requires_decoder_for_explicit_linux_binary(self):
        result = run_cli(PACKER, ["--root", str(self.repo.root), "--version", "9.9.9",
                                  "--binary", str(self.repo.binary),
                                  "--out-dir", str(self.tmp / "release-package-raw"),
                                  "--catalog-assets", str(self.assets)])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires --decoder", result.stderr)

    def test_linux_assembler_requires_split_binary_and_decoder(self):
        base = ["--repo-root", str(self.repo.root), "--version", "9.9.9",
                "--target", HOST_TARGET, "--runtime-dir", str(self.repo.root / "runtime"),
                "--catalog-assets", str(self.assets), "--out-dir", str(self.tmp / "invalid")]
        wrong_dir = self.tmp / "wrong-decoder"
        wrong_dir.mkdir()
        wrong_source = wrong_dir / "wrong.c"
        wrong_source.write_text('int main(void) { return 17; }\n')
        wrong_raw = wrong_dir / "cargo-beehive"
        subprocess.run(["gcc", "-g", "-Wl,--build-id", "-o", str(wrong_raw),
                        str(wrong_source)], check=True)
        split = run_cli(SCRIPTS_DIR / "split_debug.py", [
            "--binary", str(wrong_raw), "--shipped", str(wrong_dir / "beehive"),
            "--out", str(wrong_dir), "--version", "9.9.9", "--target", HOST_TARGET])
        self.assertEqual(split.returncode, 0, split.stderr)
        wrong_decoder = wrong_dir / self.repo.decoder.name
        for flags, error in (([], "requires explicit --binary"),
                             (["--binary", str(self.repo.binary)], "requires explicit --binary"),
                             (["--binary", str(self.repo.raw_binary),
                               "--decoder", str(self.repo.decoder)], "still has DWARF"),
                             (["--binary", str(self.repo.binary),
                               "--decoder", str(wrong_decoder)], "build ID does not match")):
            result = run_cli(ASSEMBLER, [*base, *flags])
            self.assertNotEqual(result.returncode, 0, flags)
            self.assertIn(error, result.stderr)
        self.assertFalse(list((self.tmp / "invalid").glob("*.tar.gz")))


if __name__ == "__main__":
    unittest.main(verbosity=2)
