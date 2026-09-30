# Neocloud (RunPod)

New environment → **Neocloud** rents GPUs and more from RunPod. The interface calls it Neocloud throughout and does not name the provider; code, CLI calls and these notes do. The other nine provider integrations stay in source (`src-tauri/src/neocloud.rs`, `neocloud/provider_commands.rs`, `src/lib/neocloud-providers.json`) but are not shown; existing nodes from them keep their controls.

Resource operations go through the official `runpodctl` CLI (2.x). Yougori installs it into its own data folder the first time RunPod is opened, verifying the release digest. The API key the user pastes is checked, then stored in the system credential store and passed to `runpodctl` through its environment. If `runpodctl doctor` already saved a key in `~/.runpod/config.toml`, that account is used without asking.

For `model run --neocloud`, choose 1–8 GPUs per pod (default 1). The GPU picker checks RunPod's GraphQL `lowestPrice` separately for Secure and Community Cloud, with the selected GPU count, requested disk and public SSH. Prices shown cover all selected GPUs; storage is separate. Use **Change GPU count** to check another quantity. The saved key is sent in the authorization header when present; otherwise the public inventory is used. Secure offers appear first, unavailable offers are grey, and a failed allocation disables only that GPU/cloud/count combination until Refresh. The backend repeats the filtered stock and total price check before creating a GPU pod. Stock reports are not reservations, so allocation can still fail if capacity changes. Query failures are reported as errors, not as missing stock. See [RunPod's GraphQL schema](https://graphql-spec.runpod.io/#definition-GpuLowestPriceInput).

New model processes on pods with multiple visible GPUs use Accelerate's `balanced` device map to distribute model layers, instead of pinning the model to GPU 0. This helps fit larger models; it is not tensor parallel inference and does not guarantee faster replies. Models already running keep their loaded configuration until restarted.

## What can be created

| Choice | What it is | Billing shown to the user |
|---|---|---|
| GPU pod | A template (official, community or the user's own) or any image, on a chosen GPU, 1–8 GPUs, Secure or Community Cloud, anywhere or in one data centre | Live hourly price × GPU count, a day's cost, and how long the balance lasts |
| CPU pod | A CPU template or image | RunPod picks the size; the price appears on the node once it runs |
| Serverless endpoint | A RunPod Hub repo (vLLM, ComfyUI, Whisper, embeddings…) with its required settings, GPU group or a chosen GPU, worker limits and idle time | Nothing while idle unless a worker is kept running |
| Network volume | Storage in one data centre, listed with the GPUs in stock there | About $0.07/GB a month |

Instant Clusters, Public Endpoints and savings plans are not in `runpodctl`; the home screen links to them.

The home screen also lists pods and endpoints already in the account (**Add to Yougori** attaches them), network volumes (grow, delete) and private registry logins (the password goes to `runpodctl registry create --password-stdin`).

## Pods open by themselves

1. Yougori makes its own ed25519 key (`%LOCALAPPDATA%\Yougori\neocloud\runpod-ssh`) and adds it to the RunPod account when missing. GPU pods receive account keys through RunPod-managed SSH; CPU pods are given them as `PUBLIC_KEY`.
2. Templates with Jupyter get a private `JUPYTER_PASSWORD` (kept in the credential store), so **Open Jupyter** links straight in and nobody else can.
3. After `pod create`, a watcher reads `pod get` until the pod runs and RunPod reports its public SSH port, pins the host key it presents, checks the login and Python 3, and saves the node's SSH connection. The node then shows **Open**; terminals and files work like any cloud node.
4. Starting or restarting a pod runs the watcher again, because the address and host key change. Watchers resume after Yougori restarts.

Stopping disconnects first. Deleting needs the name typed and verifies the pod is gone. A create that RunPod refuses leaves no node; one that times out keeps a node with **Check RunPod**, which attaches the pod by name or confirms nothing was created.

## Serverless endpoints

Creation uses `serverless create --hub-id`, passing only settings the user changed so the Hub's defaults (image, disk, GPU groups, CUDA) apply. **Pause** sets the worker limit to 0 and keeps the URL; **Resume** restores it. The node shows the run, runsync and (for text models) OpenAI-compatible URLs, worker and job counts, a **Try it** box (`serverless run --input -`) and a curl example that reads `$RUNPOD_API_KEY`.

## Safety

- A GPU pod is created only if the GPU is still in stock (in the chosen data centre, if any) and its current price is not above the price the user reviewed.
- Requests are argument arrays checked in `neocloud/runpod.rs`; names, images, IDs, ports and environment variables are validated before `runpodctl` runs.
- Storage prices shown are RunPod's published rates as of September 2026 (`STORAGE_RATES` in `src/api/runpod-api.ts`).

## Tests

`cargo test --lib neocloud` covers arguments, price and stock checks, Hub settings and pod states. `cargo test --lib live_catalogue -- --ignored` reads a real account without creating anything. `src/components/dialogs/neocloud-form.test.tsx` walks the connect, GPU pod, CPU pod and endpoint flows; `e2e/neocloud.spec.ts` checks the GPU pod flow in the app.
