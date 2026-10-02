//! Protected bindings contain references in project/runtime metadata, never values.
use serde_json::{json, Value};
use std::collections::BTreeMap;
const SERVICE: &str = "Yougori.DeploymentSecrets.v1";
fn entry(name: &str) -> Result<keyring::Entry, String> {
    if !yougori_cli::workload::identifier(name) { return Err("Use a secret reference with 1–80 letters, digits, dots, dashes or underscores".into()); }
    keyring::Entry::new(SERVICE, name).map_err(|_| "Cannot open the OS credential vault".into())
}
pub(crate) fn store(name: &str, value: &str) -> Result<(), String> {
    // Env-file records are one line. JSON/PEM credentials belong in protected guest files instead.
    validate_value(value)?;
    entry(name)?.set_password(value).map_err(|_| "Cannot save the deployment secret in the OS credential vault".into())
}
fn validate_value(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 65536 || value.contains(['\0', '\r', '\n']) {
        return Err("A deployment environment secret must be a nonempty single line up to 64 KiB".into());
    }
    Ok(())
}
pub(crate) fn resolve(name: &str) -> Result<String, String> {
    let value=entry(name)?.get_password().map_err(|_| format!("YOUGORI_SECRET_UNAVAILABLE: deployment secret '{name}' is unavailable in the OS credential vault; store it before applying or starting this deployment"))?;
    validate_value(&value).map_err(|_|format!("YOUGORI_SECRET_INVALID: deployment secret '{name}' must be a nonempty single line up to 64 KiB; its value was withheld"))?;
    Ok(value)
}
pub(crate) fn inject(value: &mut Value, bindings: &BTreeMap<String,String>) -> Result<(), String> { inject_with(value, bindings, resolve) }
fn inject_with(value: &mut Value, bindings: &BTreeMap<String,String>, get: impl Fn(&str)->Result<String,String>) -> Result<(), String> {
    let mut protected = BTreeMap::new();
    for (variable, reference) in bindings { protected.insert(variable, get(reference)?); }
    // Guest receives these separately and uses a 0600 env file, never --env KEY=value argv.
    value["protectedEnvironment"] = json!(protected);
    value.as_object_mut().ok_or("Invalid workload options")?.remove("secretEnvironment");
    Ok(())
}
pub(crate) fn variable(options: &yougori_cli::workload::Options, name: &str) -> Result<String,String> {
    if let Some(reference) = options.secret_environment.get(name) { return resolve(reference) }
    options.environment.get(name).cloned().ok_or("This workload has no such secret binding".into())
}

#[tauri::command]
pub fn set_deployment_secret(name: String, value: String,window:crate::WebviewWindow) -> Result<Value,String> {
    if window.label()!="main"{return Err("Configure deployment secrets from the main Yougori window or its same-user CLI".into())}
    set_secret(&name,&value)
}
pub(crate) fn set_secret(name:&str,value:&str)->Result<Value,String>{
    store(&name, &value)?;
    Ok(json!({"reference":name,"stored":true,"storage":"OS credential vault","valueReturned":false}))
}
#[tauri::command]
pub fn delete_deployment_secret(name: String,window:crate::WebviewWindow) -> Result<Value,String> {
    if window.label()!="main"{return Err("Configure deployment secrets from the main Yougori window or its same-user CLI".into())}
    delete_secret(&name)
}
pub(crate) fn delete_secret(name:&str)->Result<Value,String>{
    match entry(&name)?.delete_credential() { Ok(()) | Err(keyring::Error::NoEntry) => {}, Err(_) => return Err("Cannot remove the deployment secret from the OS credential vault".into()) }
    Ok(json!({"reference":name,"deleted":true}))
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn invalid_protected_values_cannot_inject_environment_records_or_http_headers() {
        for value in ["".to_owned(), "private\r\nAuthorization: injected".into(), "private\nOTHER=injected".into(), "private\0suffix".into(), "x".repeat(65537)] {
            let error=validate_value(&value).unwrap_err();
            assert!(!error.contains("private"));
        }
        assert!(validate_value("literal '$VALUE' = credential").is_ok());
    }
    #[test] fn bindings_do_not_persist_values_or_return_them_in_metadata() {
        let options: yougori_cli::workload::Options = serde_json::from_value(json!({"secretEnvironment":{"API_KEY":"company-api"}})).unwrap();
        assert!(!serde_json::to_string(&options).unwrap().contains("top-secret"));
        let mut wire=serde_json::to_value(&options).unwrap();
        inject_with(&mut wire, &options.secret_environment, |_| Ok("top-secret".into())).unwrap();
        assert!(wire.get("secretEnvironment").is_none());
        assert_eq!(wire["protectedEnvironment"]["API_KEY"], "top-secret");
        assert!(wire["environment"].get("API_KEY").is_none());
    }
}
