# Prime Agent

This is a heavily modified version of [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent). If you would like more information, you should check out their repository.

## Improvements

### No Prime Inference

Prime inference login is now _fully optional_.

### No Telemetry

Telemtry is _fully disabled_ at the source. 

### Daybreak

Support for OpenAI's [daybreak program](https://openai.com/daybreak/):
* Sending the daybreak blue flag on requests if supported
* Blue badges in the model selection list for daybreak blue supported models
* Blue badges in the TUI for session utilizing daybreak blue

### Web search

Support for more web search options:
* ChatGPT
* Claude

### Allowed models

Open `/settings` → **Models** to choose the main session’s model. Under **Agents**, open **Child allowed models**, type to search, and press **Space** to check or uncheck a model. **Enter** saves the whole selection; **Esc** discards the edits. Set sub-agent effort separately on the same tab. Children can query the effective list; a rejected spawn also lists the allowed choices.

Use inheritance to follow your global policy, or allow all models to remove the session restriction. Session overrides survive reattaching and apply to subsequent spawns in the same session family, not to other sessions.

### Named workflows

A workflow is a reusable job with its own model and child settings. Save a definition such as `.prime/workflows/review.json` in the session's working directory:

```json
{
  "version": 1,
  "model": "openai-codex/gpt-6.2-astra",
  "effort": "high",
  "subagents": { "allowedModels": ["openai-codex/gpt-6.2-sol"], "effort": "medium" },
  "prompt": "Review the supplied files, paying particular attention to the requested focus.",
  "inputFields": {
    "files": {"type": "array", "required": true},
    "focus": {"type": "string", "required": false}
  },
  "outputFields": {
    "summary": {"type": "string", "required": true},
    "findings": {"type": "array", "required": false}
  }
}
```

`/run review` opens a form built from `inputFields`. Enter the values and select **Run workflow**, or press **Esc** to cancel. You do not redefine the prompt or the result when you run it. From the REPL:

```python
names = await rlm.workflows.list()
definition = await rlm.workflows.describe("review")
result = await rlm.workflows.run("review", {"files": ["src/parser.rs"], "focus": "error handling"})
print(result["summary"])
```

Both field maps are required; use `{}` if there are no fields. Types are `string`, `integer`, `number`, `boolean`, `object`, `array`, and `null`. Optional means the key may be absent, not that a value of the wrong type is accepted. Missing required fields, undeclared keys and mistyped values are errors on input and output. The caller's model and session settings are unchanged. A workflow cannot expand the caller's allowed models.

`await rlm.workflows.create("review", definition)` creates a project workflow without overwriting an existing name. For an **immutable** workflow, an administrator installs the JSON under `/etc/prime-agent/workflows/` on Unix or `Prime Agent\workflows` under Program Files on Windows, without granting the agent write access. Protected names take precedence over project files. Run agents without administrative privileges; a JSON flag cannot protect an agent-writable file.

### Inbox reminders

Configure inbox reminders in `/settings` for the attached session. **Preview** (the default) shows the first five unread messages, up to 200 characters each. **Full** includes every unread message body, which can use considerably more context. **Count** shows only the unread count. None of these marks a message as read or changes when the parent wakes.

At 20 unread agent-to-agent messages, the worker bypasses the reminder setting and sends **every unread message body** directly to the recipient. An idle parent wakes; a busy parent gets a steering message. Once the batch is saved to the delivery queue, inbox space is available again.

### Inspect the harness

In an attached session, type `/harness` to browse what the agent has saved. **Local** entries belong to that session; **global** entries are shared across sessions. Select a skill to read its saved instructions and Python reference without running it. The tree refreshes while it is open. Deleting a leaf removes the saved entry after confirmation, not the file it references.

### Guardrail isolation

When a sub-agent hits a provider guardrail, Prime permanently stops that child and pauses new parent input. It runs the immutable `diagnose-guardrail` workflow once for that exact task using only `openai-codex/gpt-5.6-sol`; the account’s advertised Daybreak Blue capability is applied by the normal OpenAI request path. Codex cybersecurity blocks are classified as safety failures even when the provider supplies a generic error code, so the blocked request is not retried. If the diagnosis establishes that the underlying work is allowed, the parent resumes with the workflow’s compliant recovery plan. An uncertain, disallowed, unavailable, duplicate, or over-budget diagnosis fails closed. Diagnoses are serialized, duplicate tasks are suppressed, and each parent session is capped at three diagnostic calls.

## Install

Install the latest stable version with the one-command installer:

```bash
curl -fsSL https://app.primeintellect.ai/prime-agent/install.sh | sh
```

On Windows x64, install this fork from PowerShell:

```powershell
irm https://raw.githubusercontent.com/csmoke66/prime-agent/main/install.ps1 | iex
```

The installer downloads the fork's [published Windows build](https://github.com/csmoke66/prime-agent/releases/latest) and verifies its SHA-256 checksum. To publish the first build, enable [GitHub Actions](https://github.com/csmoke66/prime-agent/actions) on the fork and push `main` from GitHub Desktop. Wait for the Windows publish workflow to finish before running the command. No version bump, manual tag, or R2 account is needed.

## License

Prime Agent is fully open source and released under the [MIT License](LICENSE).

## Citation

If you use this codebase in your research, please cite Prime Agent:

```bibtex
@article{karten2026prime,
  title={Prime Agent: A Self-Improving RLM Harness},
  author={Karten, Seth and Zhang, Alex L. and Thomas, Kevin and Müller, Sebastian and Bakouch, Elie and Auras, Daniel and Senghaas, Mika and Obeid, Fares and Dunas, Konstantin and Hagemann, Johannes and Jaghouar, Sami},
  journal={arXiv preprint arXiv:2608.23552},
  year={2026}
}
```

Available at [https://arxiv.org/abs/2608.23552](https://arxiv.org/abs/2608.23552).
