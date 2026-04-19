# GPU Worker on Cloud Run

This directory prepares a GPU-backed `converge-llm-server` deployment for
Google Cloud Run.

## Current status

- infrastructure template: included
- deployment script: included
- runtime/backend wiring: prepared for CUDA builds

## Required inputs

- `PROJECT_ID`
- `REGION`
- `MODEL_PATH` or mounted model artifact path
- optional TLS material if you want internal mTLS

## Notes

Cloud Run GPU works best for on-demand inference workers, not long training
runs. For training, prefer GCE GPU VMs, GKE, or a dedicated GPU platform.
