# Oh-My-Prime
A self improving RML AI harness that uses a python REPL. The environment is persistent through `skills`, `heartbeats`, `workflows`, and `memory`. 

This project exists as an experiment into reducing the pain points that exist in most harnesses due to a lack of flexibility, extensibility, and usability. Instead of adding hundreds of settings, we only express core features as part of the harness itself. Everything else is defined in code and data. Your agents should be able to flourish without strict limitations and bad harness design getting in the way.

![omp](omp.png)

## Daybreak

Support for OpenAI's [daybreak program](https://openai.com/daybreak/):
* Sending the daybreak blue flag on requests if supported
* Blue badges in the model selection list for daybreak blue supported models
* Blue badges in the TUI for session utilizing daybreak blue

## Railguard handling
Specific handling settings for railguards instead of the typical fully blocking failures.

### Sanitization
Instead of exposing the scary raw blocked message to agents, we provide a sanitized version. This includes the fact that the block may be a false positive.

### Hiding
We offer the ability to hide the message altogether. You do this at your own risk.

### Routing
We offer the ability to route the blocked message details to an existing session. This allows you to asynchronously diagnose and decide on a plan of action without getting in the way of the model (such as through our default provided workflows).

Default workflows are provided with the repository to help handle false positive railguard violations:
1. `railguard-context-repair` diagnoses the reason for a session's false positives, then rewrites and compacts the context if appropriate
2. `railguard-memory-repair` diagnoses any potential false positive triggers in either `local` or `global` memory, then rewrites the memory if appropriate

These features are **not** meant to bypass railguards, and **will not** help with that. Even with the workflows a secondary model is still in the loop for these decisions.

## Named workflows

A workflow is a reusable job with its own model and child settings. Save a project definition such as `.prime/workflows/review.json` in the session's working directory, or put a shared definition in `~/.prime/agent/workflows/` to make it available from every project:

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

`/run review` opens a form built from `inputFields`. Enter the values and select **Run workflow**, or press **Esc** to cancel. To run against another existing session, use its active or durable session ID: `/run --session SESSION_ID review`. The form and workflow execute on that target session, so state-rewrite workflows are applied there. You do not redefine the prompt or result when you run it. From the REPL:

```python
names = await rlm.workflows.list()
definition = await rlm.workflows.describe("review")
result = await rlm.workflows.run("review", {"files": ["src/parser.rs"], "focus": "error handling"})
receipt = await rlm.workflows.run("review", {"files": ["src/parser.rs"]}, session="SESSION_ID")
print(result["summary"])
```

Both field maps are required; use `{}` if there are no fields. Types are `string`, `integer`, `number`, `boolean`, `object`, `array`, and `null`. Optional means the key may be absent, not that a value of the wrong type is accepted. Missing required fields, undeclared keys and mistyped values are errors on input and output. The caller's model and session settings are unchanged. A workflow cannot expand the caller's allowed models.

`await rlm.workflows.create("review", definition)` creates a project workflow without overwriting an existing name. User-global workflows under `~/.prime/agent/workflows/` are the fallback, so a project may override one for its own sessions. For an **immutable** workflow, an administrator installs the JSON under `/etc/prime-agent/workflows/` on Unix or `Prime Agent\workflows` under Program Files on Windows, without granting the agent write access. Protected operator definitions take precedence over project files, and project files take precedence over user-global files. Run agents without administrative privileges; a JSON flag cannot protect an agent-writable file.

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

## Prime-Agent

This is a heavily modified version of [Prime Agent](https://github.com/PrimeIntellect-ai/prime-agent). If you would like more information, you should check out their repository.

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
