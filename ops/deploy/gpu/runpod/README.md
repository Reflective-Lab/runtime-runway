# GPU Worker on Runpod

Use this template when you want a longer-lived or more VM-like GPU worker than
Cloud Run GPU.

Recommended use:

- persistent `converge-llm-server`
- custom CUDA images
- large model checkpoints mounted from network or volume storage
