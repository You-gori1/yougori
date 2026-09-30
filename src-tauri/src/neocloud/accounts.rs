use super::{program, run_with_timeout, token};
use serde_json::{json, Value};

pub(super) fn metadata() -> Vec<Value> {
    let mut providers: Vec<Value> =
        serde_json::from_str(include_str!("../../../src/lib/neocloud-providers.json"))
            .expect("provider metadata");
    // Keep the remaining integrations implemented, but publish RunPod alone
    // until their setup and catalogues receive the same provider-level review.
    providers.retain(|provider| provider["id"] == "runpod");
    for provider in &mut providers {
        provider["available"] = json!(true);
        provider["installAvailable"] = json!(true);
        provider["reason"] = provider["billingNote"].clone();
        if !cfg!(windows) {
            provider["requiresWsl"] = json!(false);
            if let Some(command) = provider["setupCommand"].as_str() {
                provider["setupCommand"] = json!(command.trim_start_matches("wsl --exec "));
            }
        }
    }
    providers
}

pub(super) fn key_variable(provider: &str) -> Option<&'static str> {
    match provider {
        "runpod" => Some("RUNPOD_API_KEY"),
        "vast" => Some("VAST_API_KEY"),
        "jarvis" => Some("JL_API_KEY"),
        "prime" => Some("PRIME_API_KEY"),
        "thunder" => Some("TNR_API_TOKEN"),
        "latitude" => Some("LATITUDESH_TOKEN"),
        "civo" => Some("CIVO_TOKEN"),
        _ => None,
    }
}

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    program(provider)?;
    keyring::Entry::new("Yougori.Neocloud.v1", provider)
        .map_err(|_| "System credential store is unavailable".into())
}

pub(super) fn saved_key(provider: &str) -> Result<Option<String>, String> {
    if key_variable(provider).is_none() {
        return Ok(None);
    }
    match entry(provider)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => {
            Err("Cannot read the provider credential from the system credential store".into())
        }
    }
}

pub(super) fn check_args(provider: &str, location: Option<&str>) -> Result<Vec<String>, String> {
    let args: Vec<&str> = match provider {
        "runpod" => vec!["user"],
        "vast" => vec!["show", "user", "--raw"],
        "jarvis" => vec!["status", "--json"],
        "crusoe" => vec!["whoami", "--json"],
        "prime" => vec!["pods", "list", "--output", "json"],
        "thunder" => vec!["status", "--json"],
        "latitude" => vec!["servers", "list", "-o", "json"],
        "civo" => vec!["quota", "show", "--output", "json"],
        "nebius" => {
            let project = location
                .filter(|s| token(s, 128))
                .ok_or("Choose a Nebius project ID to verify access")?;
            vec![
                "compute",
                "instance",
                "list",
                "--parent-id",
                project,
                "--format",
                "json",
            ]
        }
        "e2e" => {
            let (project, region) = super::provider_commands::e2e_context(location.unwrap_or(""))?;
            vec![
                "--project_id",
                project,
                "--location",
                region,
                "node",
                "list",
            ]
        }
        _ => return Err("Unknown provider".into()),
    };
    Ok(args.into_iter().map(str::to_owned).collect())
}

#[tauri::command]
pub async fn neocloud_account(provider: String, location: Option<String>) -> Result<Value, String> {
    program(&provider)?;
    if let Err(error) = run_with_timeout(&provider, &["--help".into()], 15).await {
        return Ok(
            json!({"provider":provider,"status":"cliUnavailable","message":error,"checkedAt":chrono::Utc::now().to_rfc3339()}),
        );
    }
    let args = match check_args(&provider, location.as_deref()) {
        Ok(args) => args,
        Err(message) => {
            return Ok(json!({"provider":provider,"status":"needsContext","message":message}))
        }
    };
    let result = run_with_timeout(&provider, &args, 30).await;
    Ok(match result {
        Ok(value) if value.is_object() || value.is_array() => {
            json!({"provider":provider,"status":"ready","message":"Account access verified with the provider CLI","checkedAt":chrono::Utc::now().to_rfc3339()})
        }
        Ok(_) => {
            json!({"provider":provider,"status":"signInRequired","message":"The CLI did not return account data. Complete sign-in and update the CLI."})
        }
        Err(message) => {
            json!({"provider":provider,"status":"signInRequired","message":message,"checkedAt":chrono::Utc::now().to_rfc3339()})
        }
    })
}

#[tauri::command]
pub async fn neocloud_authenticate(
    provider: String,
    api_key: String,
    location: Option<String>,
) -> Result<Value, String> {
    key_variable(&provider).ok_or("Use this provider's interactive CLI sign-in")?;
    if api_key.trim().len() < 8 || api_key.len() > 8192 || api_key.chars().any(char::is_control) {
        return Err("Enter a valid provider API key".into());
    }
    let args = check_args(&provider, location.as_deref())?;
    // Validate before replacing a working saved credential. The key is passed
    // only through this child's environment, never shell arguments or state JSON.
    let value = super::run_authenticated(&provider, &args, 30, Some(api_key.trim())).await?;
    if !value.is_object() && !value.is_array() {
        return Err("The CLI did not return account data. Check the key and CLI version.".into());
    }
    entry(&provider)?
        .set_password(api_key.trim())
        .map_err(|_| "Could not save the API key in the system credential store")?;
    Ok(
        json!({"provider":provider,"status":"ready","message":"Signed in; API key saved in the system credential store"}),
    )
}

#[tauri::command]
pub fn neocloud_forget_account(provider: String) -> Result<(), String> {
    match entry(&provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("Could not remove the saved provider key".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runpod_is_the_only_published_provider_and_others_remain_in_metadata() {
        let providers = metadata();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0]["id"], "runpod");
        let stored: Vec<Value> =
            serde_json::from_str(include_str!("../../../src/lib/neocloud-providers.json")).unwrap();
        assert_eq!(stored.len(), 10);
        let mut ids = std::collections::HashSet::new();
        for p in stored {
            let id = p["id"].as_str().unwrap();
            assert!(ids.insert(id.to_string()));
            assert!(program(id).is_ok());
            assert!(p["pricingUrl"].as_str().unwrap().starts_with("https://"));
            assert!(!p["setupCommand"].as_str().unwrap().is_empty());
            assert!(p["products"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p == "gpu" || p == "cpu"));
        }
    }
    #[test]
    fn readiness_is_read_only_and_nebius_requires_a_project() {
        assert!(check_args("nebius", None).is_err());
        assert!(check_args("nebius", Some("--bad")).is_err());
        for p in metadata() {
            let id = p["id"].as_str().unwrap();
            let args = check_args(
                id,
                Some(if id == "e2e" {
                    "123:Delhi"
                } else {
                    "project-1"
                }),
            )
            .unwrap();
            assert!(!args
                .iter()
                .any(|a| matches!(a.as_str(), "create" | "delete" | "start" | "stop")));
        }
    }
}
