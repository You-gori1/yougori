# Yougori failure-testing campaign — 2026-10-02

Nine specialist agents and the final integration pass tested the desktop, CLI,
installers, control transport, transfers, guest agent, project/database sync,
providers, readiness, models, vault, and website packaging. Confirmed defects
were fixed in the source checkout. Tests included real disposable OCI workloads,
CUDA crash recovery on D:, native Windows pipes and terminals, Linux filesystem
fixtures, and the physical Raspberry Pi.

The source baseline was public-main `70950efbf3dfb8c7cc7ea05c61a5f64ac019969e`,
version 1.0.6. At the end of the audit, no new release, installer replacement,
GitHub push, website deployment, or installed engine update had been performed.
Historical website edits were preserved.

The subsequent source-publication commit includes the tested rebuilt agent in
the checked-in OCI initramfs and CUDA payload, with refreshed checksum manifests,
so the new host and guest code travel together. This does not replace installed
apps or publish new downloadable installers. The website publication contains
only its three audit fixes, applied to the current private main.

## Final automated checks

| Check | Result | Scope |
| --- | --- | --- |
| Native Rust library | 407 passed; 80 explicitly ignored | Control coordination, Windows pipe saturation, cancellation, journal/output, storage/recovery, readiness, sharing, providers and native components |
| Native all-targets check | Passed | Final desktop/native source and its CLI/vault dependencies |
| CLI Rust tests | 374 passed; 5 explicitly ignored | Library, current and legacy binaries, standalone-process tests and preference-write preservation |
| CLI all-targets check | Passed | Final CLI source |
| Node script tests | 181 passed; 5 Windows-specific skips | Installers, launcher/sync/SQLite, release and packaging scripts |
| React/Vitest | 551 passed in 91 files | Final frontend, including five new terminal failure scenarios |
| Chromium browser suite | 191 passed | Source test adapter; desktop/mobile layouts, onboarding, guides, graph/list management, sharing, terminals, theme, models, cloud forms and startup UI |
| Go guest race suite | 105 top-level tests; 135 passing events including subtests; no failures | Final guest source, firmware credentials, cancellation, archive framing, filesystem guards, execution receipts and emulation budgets |
| Go vet | Passed | Final guest source |
| Linux private firmware file | Passed | Exact Rust source tested under Linux: mode 0600, Unicode/comma path, secret-free argument, invalid-input refusal and owned-file cleanup |
| Real PowerShell fixtures | 2 passed | Candidate CLI, Unicode/input/resize/Ctrl+C, selected-tab cleanup and descendant cleanup after host termination |
| Linux sync/SQLite regressions | 28 passed; no skips | Real Linux directory replacement, staging-file replacement, WAL commits, large files and bounded protocol replies |
| Vault | 29 passed; 1 explicitly ignored; all-targets check passed | Binding/replay protection, crypto/tampering, OAuth, TLS, redaction and DNS validation |
| CUDA host | 11 passed; 5 explicitly ignored | Provider ownership, state and recovery |
| Model checks | 22 server, 5 real tiny-checkpoint, 3 chat-client and 5 Linux process/API tests passed | Frozen-worker fixtures, HTTP failure/redaction, immutable revisions, tiny CPU Qwen 3.5/Llama checkpoints and isolated helper reuse |
| Website retention/counters | 6 Node checks; 11 Linux cleanup checks passed in normal and Python `-O` modes | Temporary package/counter fixtures; corruption and symlink refusal; exact inventory generation |
| ESLint, TypeScript and Vite | Passed | Final frontend production build |
| Production npm dependency audit | No reported vulnerabilities | Production npm dependencies only; this is not a complete supply-chain audit |
| Whitespace checks | Passed in both repositories | Changed app and website source |

Ignored tests are not counted as passing. Separate real tests below cover some
ignored native/provider fixtures. Existing Vite mixed-import warnings do not
fail the build. Early combined browser runs failed during cold module loading
and source changes; the final stable-source full run passed all 191 assertions.
The Edit App test now waits for the workspace to load before asserting editor
controls; its editor assertions were retained.

## Confirmed source defects fixed

| Area | Defect and resulting behavior |
| --- | --- |
| Windows control transport | More than 32 simultaneous pipe connections could permanently close the listener. Accepted streams now have bounded admission/lifetimes, and listener replacement remains available under saturation. |
| Shared appliance isolation | An ordinary container could read the appliance control credential from `/proc/cmdline`. Shared OCI appliances now receive it through QEMU firmware and a protected host file created with a user-only Windows DACL or Unix mode 0600. The kernel and QEMU arguments contain no credential value. Containers cannot read the firmware source. |
| Transfer outcomes | A request's total deadline was reported as an ordinary failure despite possible partial extraction. The structured deadline outcome now preserves the partial-copy policy. |
| Oversized output | UTF-8 truncation could advertise bytes that were not actually pageable. Output metadata now reflects retrievable bytes while execution remains complete. |
| Snapshots and Stop | A stalled snapshot could retain the lifecycle lock. Guest and host export registries coordinate exact cancellation before Stop/restart/delete acquire their locks; cancellation also releases staging and resumes only a snapshot-paused workload. |
| Import receipts | EOF after a file, one zero block, or orphan PAX/GNU metadata could be acknowledged as a complete archive. Actual aligned terminator headers are required, metadata payload zeros cannot impersonate them, and the whole declared body is checked. |
| Remote saves | Overlapping physical roots could both accept a stale expected checksum. Conditional saves now serialize by verified physical parent/file identity. |
| Execution completion | Delayed provider receipts could make a completed command appear timed out. Completion preserves the actual guest result. |
| Older container providers | Legacy synchronous execution returned provider exit 1 for any guest failure and appended provider diagnostics. A private completion receipt now preserves the real guest exit and stderr; a missing receipt reports an unknown outcome without retrying. |
| ARM emulation | Fixed native-speed startup limits rejected a progressing x86 appliance/container on ARM. Emulated startup has a finite larger budget and serial-progress/inactivity diagnostics; Stop retains its shorter bound. |
| Sync preferences | Invalid UTF-8/unreadable preference files could be replaced with a fresh template. Only absence permits creation; existing bytes survive read failures. |
| Project sync filenames | `__proto__` and similar filenames could disappear from checksum/error objects. Replies now retain their own keys. |
| Sync staging | A replaced symlink, ordinary parent directory, or staging filename could redirect writes/cleanup to unrelated files. Uploads and SQLite batches retain their descriptors and verify parent/staging identities before use or unlink. Ambiguous cleanup preserves the file. |
| CLI secret arguments | Interspersed global flags could bypass refusal of shell-argument deployment secrets. Validation uses normalized positional words. |
| CLI engine identity | Null/scalar/array identity replies could be accepted or panic. Invalid identity shapes now return normal structured failures. |
| Personal preferences | Remember/forget could edit personal notes outside the exact managed section; large confirmation counts could overflow. Section boundaries and counts are handled safely. |
| Installer input | Insecure/credential-bearing URLs, invalid Windows overrides and malformed checksums could reach download handling. Inputs fail preflight without disclosing credential fixtures. |
| Download-link UI | Stale heartbeats could restore revoked links, partial failures retained successful removals, and busy state ended before other revocations finished. Revision checks and per-link/all-settled outcomes preserve current state. |
| Mobile notifications | A toast could cover Theme and keep its own timeout paused indefinitely. Message text permits clicks through while notification actions/dismissal remain accessible. |
| Host terminal | A transient read failure permanently stopped polling and disabled a live terminal. Bounded read retries retain the cursor; writes are never retried. |
| Private network bridge | Heartbeats/WebSocket events could cancel a partially consumed length prefix/body and lose framing. Incremental receiver state survives those interruptions. The loopback regression failed before this fix and passes afterward. |
| Readiness state | Corrupt health configuration silently became “no probe.” Corruption now fails closed and remains available for reconciliation. |
| Health requests | Local/public probes encoded Unicode/spaces differently and accepted malformed status lines/paths. Both enforce the same origin/path semantics and validate protocol/status syntax. |
| Publication metadata | Loopback listeners advertised unreachable LAN/guest-network URLs. Only addresses allowed by their actual bind are reported. |
| Model generation | Frozen first generation steps could hold streaming/nonstreaming HTTP workers indefinitely. Wall-clock failures request cancellation, retain exclusive compute ownership until the old worker exits, and report an unhealthy worker for restart. |
| Model revisions | Missing, malformed or mismatching immutable revisions could reach weight loading. They are rejected before downloading/loading. |
| Model accounting | Malformed persisted hour keys could break usage accounting. Bounded ASCII syntax and defensive pruning handle corrupted entries. |
| Model error output | Validation/type errors could disclose the model API token. Responses redact it. |
| Model startup | The expanded model server exceeded the guest's existing 32 KiB command limit. A standard-library zlib bootstrap preserves the exact script within that limit, including refresh of previously saved model commands. |
| Vault DNS policy | CNAME targets with invalid labels/dots/hyphens passed validation. Consistent label validation rejects them while retaining legitimate record-name forms. |
| Website cleanup | Corrupt current packages could cause older packages to be deleted. Cleanup requires a complete URL/size/SHA-256 inventory, including MSI, verifies current payloads, and rejects symlink/ambiguous inputs before deletion. |

A flaky cloud-export failure test also conflated Python startup delay with
transfer inactivity. It now establishes actual received bytes before advancing
the inactivity clock; real owned-process termination and staging assertions
remain intact.

The real host-terminal fixture had still asserted the pre-1.0.6 direct schema
shape. It now checks the current version-2 envelope and can use an explicit
candidate CLI, rather than silently depending on a stale bundled executable.

## Real failure tests and preserved state

- **Updated private OCI appliance:** real container execution preserved exit 17,
  Unicode and stderr. Complete binary import preserved bytes and mode 0600;
  missing terminators and zero-filled orphan GNU metadata were rejected. An
  HTTP relay then stopped reading a real 128 MiB snapshot. Exact export
  cancellation plus Stop finished in **2.101 seconds**. Restart preserved the
  original checksum, another environment remained running with its marker,
  and no cancelled host archive was published. The isolation regression first
  failed against the credential-bearing kernel command line; with the new boot
  path, the real container could read neither that credential nor its firmware
  source. Full fixture teardown passed.
- **CUDA on D:** a fresh private WSL registration executed a GPU kernel,
  rejected two live owners, survived interruption of its test engine, recovered
  verified abandonment, and restarted with data and `/dev/dxg` intact. Only
  that registration was removed. This tested current CUDA host source with the
  earlier bundled CUDA guest payload, not every new OCI guest change.
- **OCI/VM ownership:** private orphan/live-owner tests preserved live processes
  and unrelated disks; only verified owned abandoned fixtures were reclaimed.
- **Physical Pi 4, ARM64 Debian:** the installed 1.0.6 native CLI connected and
  was inspected. A disposable CLI-created container hit the existing 90-second
  guest start timeout twice; Stop and confirmed removal of only that fixture
  succeeded. A second, completely separate QEMU overlay backed by the immutable
  base booted the patched guest in **822 seconds**, provisioned in **288 seconds**
  and started in **170 seconds**. It exposed the older-provider exit-code defect.
  That fix subsequently passed Go regressions and the real updated OCI test
  above; it and the newer firmware-credential change were **not** retested or
  installed on the Pi. The private QEMU exited,
  its exact temporary directory was removed, and all three original environment
  IDs were verified unchanged. X86 software emulation remains slow on this Pi;
  larger bounded budgets improve correctness, not emulation speed.
- **Windows terminals:** dedicated native PowerShell fixtures exercise Unicode
  paths/output, offline candidate CLI commands, resize, Ctrl+C, selected-tab
  cleanup, and independent-tab survival. A separate fixture covers descendant
  process cleanup after termination of its own test host.

The firmware change applies to the shared OCI appliance. Dedicated microVMs
retain the legacy boot credential path: their full-root guest owns the whole
runtime, rather than sharing that kernel with unrelated containers. Existing
installed appliances were not changed. Applying the fix in a future release
requires the updated guest payload and an appliance restart.

## Coverage limits and evidence

These tests do not prove every feature or external failure mode. No macOS
hardware, paid cloud operations, real account-domain transition, OS sign-in
reboot, destructive disk-failure injection, physical GPU freeze, or every
third-party model/SDK was tested. Browser cloud/domain flows use test adapters;
public readiness error/redirect behavior uses controlled streams/TLS fixtures.
Actual model checkpoints in this campaign were small random CPU models, not
large public weights. Installer tests use owned stub downloads/engines and
temporary homes; real installer replacement was not performed.

The live website received read-only metadata/HEAD checks: six configured assets
returned matching expected sizes. These checks neither establish their live
SHA-256 hashes nor increment download counters. Website packaging changes are
local and must be included in a later deployment.

Local evidence is under `artifacts/ultra-audit-20261002/` and
`artifacts/pi-physical-20261002/`. Relevant final logs include
`browser-final.log`, `integrated-final.log`, `native-final.log`,
`native-check-final.log`, `cli/final.log`, `guest/final-race.jsonl`,
`sync/identity-linux.log`, `control/snapshot-stop-final-candidate.log`,
`terminal-browser/powershell-final.log`, `terminal-browser/powershell-exit-final.log`,
`control/isolation-cmdline.log`, `terminal-browser/bridge-before.log`, and
`terminal-browser/bridge-after.log`.
Specialist reports distinguish simulated fixtures from actual provider tests.
