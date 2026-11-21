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

`/model` lets you choose the main session's model from the full catalog. Sub-agents use a separate allowlist: they can query which models are allowed, and a rejected spawn lists the allowed choices.

In an attached session, `/subagent-settings` shows the effective child model list and effort. For example:

```text
/subagent-settings models openai-codex/gpt-6.2-astra openai-codex/gpt-6.2-sol
/subagent-settings effort high
```

Use `models all` for unrestricted child selection or `models inherit` to use `settings.json` again. `effort inherit` restores the normal spawn or parent effort. These overrides survive reattaching, apply to subsequent spawns in that session family, and leave other sessions alone.

### Inbox reminders

Use `/inbox-reminders` to see the setting for the attached session. `preview` (the default) shows the first five unread messages, up to 200 characters each. `/inbox-reminders full` includes every unread message body in each reminder, which can use considerably more context. `/inbox-reminders count` restores the count-only notice. None of these marks a message as read or changes when the parent wakes.

At 20 unread agent-to-agent messages, the worker bypasses the reminder setting and sends **every unread message body** directly to the recipient. An idle parent wakes; a busy parent gets a steering message. Once the batch is saved to the delivery queue, inbox space is available again.

### Inspect the harness

In an attached session, type `/harness` to browse what the agent has saved. **Local** entries belong to that session; **global** entries are shared across sessions. Select a skill to read its saved instructions and Python reference without running it. The tree refreshes while it is open. Deleting a leaf removes the saved entry after confirmation, not the file it references.

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
