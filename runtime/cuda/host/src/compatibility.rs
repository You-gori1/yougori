//! Read-only prerequisites. Never boots a distro, installs a driver, changes
//! Windows features, or mistakes a GPU name for a successful CUDA workload.
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}
fn check(name: &str, passed: bool, detail: impl Into<String>) -> Check {
    Check { name: name.into(), passed, detail: detail.into() }
}

#[cfg(any(windows, test))]
fn platform(architecture: &str, build: u32) -> Check {
    let passed = architecture.eq_ignore_ascii_case("AMD64") && build >= 19044;
    check("Windows", passed, if passed {
        format!("Windows x64 · build {build}. Intel and AMD CPUs are both supported.")
    } else {
        "This engine needs Windows 10 build 19044+ or Windows 11 on an Intel/AMD x64 PC. ARM, macOS and native Linux hosts do not have a Yougori GPU backend yet.".into()
    })
}

#[cfg(any(windows, test))]
fn nvidia(csv: &str) -> Check {
    let mut compatible = Vec::new();
    for line in csv.lines() {
        let columns: Vec<_> = line.split(',').map(str::trim).collect();
        if columns.len() != 5 { continue; }
        let driver = columns[1].split('.').next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let compute = columns[2].parse::<f32>().unwrap_or(0.0);
        // WSL requires Pascal or later in WDDM mode, with an R495+ driver.
        // Application/toolkit versions can impose a newer requirement.
        if driver >= 495 && compute >= 6.0 && columns[3].eq_ignore_ascii_case("WDDM") {
            let memory = columns[4].parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
                .map(|v| format!(" · {:.1} GB GPU memory", v / 1024.0)).unwrap_or_default();
            compatible.push(format!("{} · driver {}{}", columns[0], columns[1], memory));
        }
    }
    if compatible.is_empty() {
        check("NVIDIA GPU and driver", false, "No compatible NVIDIA WDDM device was verified. Use a Pascal-or-newer NVIDIA GPU with an up-to-date Windows NVIDIA driver (R495+). AMD/Intel-only GPUs cannot run CUDA. If you have NVIDIA hardware, update its Windows driver and recheck; do not install a Linux GPU driver inside the container.")
    } else {
        check("NVIDIA GPU and driver", true, compatible.join("; "))
    }
}

#[cfg(windows)]
async fn output(program: &str, args: &[&str]) -> Result<String, String> {
    use std::{process::Stdio, time::Duration};
    let mut command = super::hidden(program);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(10), command.output()).await
        .map_err(|_| "Check timed out".to_string())?.map_err(|e| e.to_string())?;
    if !result.status.success() { return Err("Prerequisite check failed".into()); }
    // WSL emits UTF-16 on some Windows versions. Its exit code is the check;
    // NVIDIA's CSV and PowerShell's fixed ASCII JSON do not need translation.
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

pub async fn checks() -> Vec<Check> {
    #[cfg(not(windows))]
    { vec![check("Operating system", false, "Yougori GPU environments currently require Windows x64 with WSL 2 and an NVIDIA GPU. Native Linux, macOS and ARM hosts are not implemented; standard environments remain available.")] }
    #[cfg(windows)]
    {
        let (host, gpu, wsl) = tokio::join!(
            output("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", r#"$a = [Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITEW6432'); if (!$a) { $a = [Environment]::GetEnvironmentVariable('PROCESSOR_ARCHITECTURE') }; $b = [Microsoft.Win32.Registry]::GetValue('HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows NT\CurrentVersion', 'CurrentBuildNumber', '0'); '{"architecture":"' + $a + '","build":' + $b + '}'"#]),
            output("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", r#"$p = (Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue).Source; if (!$p) { $p = Join-Path $env:ProgramFiles 'NVIDIA Corporation\NVSMI\nvidia-smi.exe' }; if (!(Test-Path -LiteralPath $p)) { exit 1 }; & $p --query-gpu=name,driver_version,compute_cap,driver_model.current,memory.total --format=csv,noheader,nounits; exit $LASTEXITCODE"#]),
            output("wsl.exe", &["--status"]),
        );
        let host = host.ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        let platform = platform(host.as_ref().and_then(|v| v["architecture"].as_str()).unwrap_or("unknown"), host.as_ref().and_then(|v| v["build"].as_u64()).unwrap_or(0) as u32);
        let gpu = match gpu { Ok(csv) => nvidia(&csv), Err(_) => nvidia("") };
        let wsl = check("WSL", wsl.is_ok(), if wsl.is_ok() {
            "WSL responds. Setup uses WSL 2; GPU visibility is verified inside the runtime when it starts."
        } else {
            "WSL is missing or not ready. In administrator PowerShell, run wsl --install --no-distribution, restart Windows, then run wsl --update. Enable CPU virtualization in BIOS/UEFI if WSL reports it disabled. Recheck afterwards."
        });
        vec![platform, gpu, wsl]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_multiple_generations_not_one_laptop() {
        for (name, cc, driver) in [("GeForce GTX 1060", "6.1", "536.23"), ("GeForce RTX 2060", "7.5", "551.23"), ("GeForce RTX 4090", "8.9", "572.83"), ("GeForce RTX 5090 Laptop GPU", "12.0", "592.01")] {
            assert!(nvidia(&format!("{name}, {driver}, {cc}, WDDM, 6144")).passed);
        }
    }
    #[test]
    fn rejects_unsupported_unknown_and_old_drivers() {
        for csv in ["", "not a gpu", "GTX 980, 592.01, 5.2, WDDM, 4096", "RTX 3090, 470.00, 8.6, WDDM, 24576", "Tesla, 592.01, 8.0, TCC, 8192", "NVIDIA, N/A, N/A, WDDM, N/A"] { assert!(!nvidia(csv).passed, "{csv}"); }
        assert!(nvidia("old, 592.01, 5.2, WDDM, 2048\nRTX 3060, 572.83, 8.6, WDDM, 12288").passed);
        assert!(!platform("ARM64", 26200).passed);
        assert!(!platform("AMD64", 18363).passed);
        assert!(platform("AMD64", 19044).passed);
        assert!(platform("AMD64", 26200).passed);
    }
    #[tokio::test]
    #[ignore = "Read-only Windows NVIDIA/WSL prerequisite checks"]
    async fn hardware_prerequisites() {
        let results = checks().await;
        for result in &results { println!("{}: {} — {}", result.name, result.passed, result.detail); }
        assert!(results.iter().all(|c| c.passed));
    }
}
