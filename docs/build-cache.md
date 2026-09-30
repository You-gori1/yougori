# Development build storage

Cargo stores compiled dependencies and compiler state in `target` directories.
These are development files; running the packaged Yougori application does not
create Cargo caches. The repository's `.cargo/config.toml` disables incremental
compilation and uses line tables for development/test debug information. This
keeps useful backtrace locations without full variable/type debug information.
Local recompilation can take longer. Release settings are unchanged.

Run Cargo from this checkout or one of its subdirectories so it reads this
configuration. Explicit Cargo environment or command-line overrides can change
these defaults. See the [Cargo profile documentation](https://doc.rust-lang.org/cargo/reference/profiles.html).

## Inspect and reclaim existing caches on Windows

Requires PowerShell 7 (`pwsh`). Stop Rust builds first; the app may keep running.

```powershell
npm run cache:report
npm run cache:clean
```

If target directories have been moved to another drive using junctions, explicitly
include that build-cache root. For example:

```powershell
npm run cache:report -- -CacheRoot D:\YougoriBuildCache
npm run cache:clean -- -CacheRoot D:\YougoriBuildCache
```

The report shows reclaimable bytes. Cleanup removes only Cargo's `incremental`,
`deps`, `build`, and `.fingerprint` directories in recognized build profiles.
It verifies resolved paths, rejects links inside those directories, and holds
the Cargo profile lock during deletion. It preserves top-level executables,
runtime bundles, model downloads, VM disks, credentials, and backups. The next
Rust build recreates dependencies as needed. Files locked by another process
cause cleanup to stop with an error rather than terminating that process.

Disabling incremental compilation prevents that specific cache from growing;
Cargo can still retain older dependency outputs after toolchain, dependency, or
build-flag changes. Use the report and cleanup commands when changing these.
There is no automatic deletion of model or environment data and no hard size cap
on all build artifacts.
