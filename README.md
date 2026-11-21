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

Better support for allowed models:
* `/models` only shows allowed models
* Agents can only see allowed models when listing models
* Allowed models are listed if an agent attempts to spawn a non-allowed model.

## Install

Install the latest stable version with the one-command installer:

```bash
curl -fsSL https://app.primeintellect.ai/prime-agent/install.sh | sh
```

On Windows, install from PowerShell:

```powershell
irm https://app.primeintellect.ai/prime-agent/install.ps1 | iex
```

The Windows installer downloads the latest stable Windows x64 build and verifies its checksum before installation.

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
