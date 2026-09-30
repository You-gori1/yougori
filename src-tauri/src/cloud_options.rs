//! Read-only lookups that fill in a cloud deployment: regions, images, machine types, subnets,
//! security groups, key pairs and resource groups, through the provider's signed-in official CLI.
//! Nothing is created or changed. Local SSH public keys come from ~/.ssh and never leave the PC.
use crate::cloud_deployment::{run_with_timeout, strings, word};
use serde_json::{json, Value};

pub const KINDS: &[&str] = &["regions", "images", "machineTypes", "subnets", "securityGroups", "keyPairs", "resourceGroups", "sshKeys"];

fn item(id: &str, name: &str, detail: impl Into<String>) -> Value {
    json!({"id": id, "name": name, "detail": detail.into()})
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

/// The deploy request field each kind fills.
pub fn field(kind: &str) -> &'static str {
    match kind {
        "regions" => "region",
        "images" => "image",
        "machineTypes" => "machineType",
        "subnets" => "subnet",
        "securityGroups" => "securityGroup",
        "keyPairs" => "keyPair",
        "resourceGroups" => "resourceGroup",
        _ => "sshPublicKey",
    }
}

/// Provider CLI arguments for one lookup. AWS ships a real executable, so JMESPath `--query`
/// keeps its output small; Azure and Google use batch launchers on Windows, so their full JSON
/// is trimmed in `items` instead.
pub fn plan(provider: &str, account: &str, region: &str, kind: &str) -> Result<Vec<Vec<String>>, String> {
    let needs_region = matches!((provider, kind), (_, "machineTypes") | ("google", "subnets"));
    if needs_region && region.is_empty() {
        return Err(format!("Choose a {} first", if provider == "google" { "zone" } else { "region" }));
    }
    let aws = |args: &[&str]| {
        let mut args = strings(args);
        args.extend(strings(&["--profile", account, "--output", "json"]));
        if !region.is_empty() { args.extend(strings(&["--region", region])); }
        args
    };
    let azure = |args: &[&str]| { let mut args = strings(args); args.extend(strings(&["--subscription", account, "--output", "json", "--only-show-errors"])); args };
    let google = |args: &[&str]| { let mut args = strings(args); args.extend(strings(&["--project", account, "--format=json", "--quiet"])); args };
    Ok(match (provider, kind) {
        ("aws", "regions") => vec![aws(&["ec2", "describe-regions", "--query", "Regions[].RegionName"])],
        // Canonical publishes the current AMI IDs as public SSM parameters (24.04 on gp3, 22.04 on gp2).
        ("aws", "images") => [("24.04", "gp3"), ("22.04", "gp2")].iter().map(|(release, disk)| aws(&["ssm", "get-parameters", "--names",
            &format!("/aws/service/canonical/ubuntu/server/{release}/stable/current/amd64/hvm/ebs-{disk}/ami-id"), "--query", "Parameters[].{name:Name,id:Value}"])).collect(),
        ("aws", "machineTypes") => vec![aws(&["ec2", "describe-instance-types", "--filters", "Name=current-generation,Values=true", "Name=processor-info.supported-architecture,Values=x86_64",
            "--query", "InstanceTypes[].{id:InstanceType,cpus:VCpuInfo.DefaultVCpus,memory:MemoryInfo.SizeInMiB}"])],
        ("aws", "subnets") => vec![aws(&["ec2", "describe-subnets", "--query", "Subnets[].{id:SubnetId,vpc:VpcId,zone:AvailabilityZone,cidr:CidrBlock,public:MapPublicIpOnLaunch,tags:Tags}"])],
        ("aws", "securityGroups") => vec![aws(&["ec2", "describe-security-groups", "--query", "SecurityGroups[].{id:GroupId,name:GroupName,vpc:VpcId,description:Description}"])],
        ("aws", "keyPairs") => vec![aws(&["ec2", "describe-key-pairs", "--query", "KeyPairs[].{id:KeyName,type:KeyType}"])],
        ("azure", "regions") => vec![azure(&["account", "list-locations"])],
        ("azure", "images") => vec![azure(&["vm", "image", "list"])],
        ("azure", "machineTypes") => vec![azure(&["vm", "list-sizes", "--location", region])],
        ("azure", "subnets") => vec![azure(&["network", "vnet", "list"])],
        ("azure", "securityGroups") => vec![azure(&["network", "nsg", "list"])],
        ("azure", "resourceGroups") => vec![azure(&["group", "list"])],
        ("google", "regions") => vec![google(&["compute", "zones", "list"])],
        ("google", "images") => ["ubuntu-2404-lts-amd64", "ubuntu-2204-lts"].iter().map(|family| strings(&["compute", "images", "describe-from-family", family, "--project", "ubuntu-os-cloud", "--format=json", "--quiet"])).collect(),
        ("google", "machineTypes") => vec![google(&["compute", "machine-types", "list", "--zones", region])],
        ("google", "subnets") => vec![google(&["compute", "networks", "subnets", "list"])],
        (_, "keyPairs") => return Err("Only AWS uses key pairs; Azure and Google Cloud take an SSH public key (kind sshKeys)".into()),
        (_, "resourceGroups") => return Err("Only Azure uses resource groups".into()),
        ("google", "securityGroups") => return Err("Google Cloud uses the subnet's network firewall rules; there is no security group to choose".into()),
        _ => return Err(format!("Unknown kind {kind}; use one of {}", KINDS.join(", "))),
    })
}

/// Normalises provider output into `{id, name, detail}` items, where `id` is the exact value
/// the deploy request expects.
pub fn items(provider: &str, kind: &str, region: &str, outputs: &[Value]) -> Vec<Value> {
    let rows = || outputs.iter().flat_map(|output| match output { Value::Array(rows) => rows.clone(), other => vec![other.clone()] });
    let mut items: Vec<Value> = match (provider, kind) {
        ("aws", "regions") => rows().filter_map(|r| r.as_str().map(|id| item(id, id, ""))).collect(),
        ("aws", "images") => rows().map(|r| {
            let release = if text(&r, "name").contains("/24.04/") { "Ubuntu 24.04 LTS" } else { "Ubuntu 22.04 LTS" };
            item(text(&r, "id"), release, "Current Canonical image (amd64)")
        }).collect(),
        ("aws", "machineTypes") => rows().map(|r| item(text(&r, "id"), text(&r, "id"), format!("{} vCPU · {:.1} GB", r["cpus"], r["memory"].as_f64().unwrap_or(0.0) / 1024.0))).collect(),
        ("aws", "subnets") => rows().map(|r| {
            let name = r["tags"].as_array().into_iter().flatten().find(|t| t["Key"] == "Name").and_then(|t| t["Value"].as_str()).unwrap_or("");
            item(text(&r, "id"), if name.is_empty() { text(&r, "id") } else { name }, format!("{} · {} · {}{}", text(&r, "zone"), text(&r, "cidr"), text(&r, "vpc"), if r["public"] == true { " · public IPs" } else { "" }))
        }).collect(),
        ("aws", "securityGroups") => rows().map(|r| item(text(&r, "id"), text(&r, "name"), format!("{} · {}", text(&r, "vpc"), text(&r, "description")))).collect(),
        ("aws", "keyPairs") => rows().map(|r| item(text(&r, "id"), text(&r, "id"), text(&r, "type").to_owned())).collect(),
        ("azure", "regions") => rows().filter(|r| r["metadata"]["regionType"] == "Physical").map(|r| item(text(&r, "name"), text(&r, "displayName"), text(&r["metadata"], "geography").to_owned())).collect(),
        ("azure", "images") => rows().filter(|r| !text(r, "urn").is_empty()).map(|r| item(text(&r, "urn"), text(&r, "urnAlias"), format!("{} {}", text(&r, "publisher"), text(&r, "offer")))).collect(),
        ("azure", "machineTypes") => rows().map(|r| item(text(&r, "name"), text(&r, "name"), format!("{} vCPU · {:.1} GB", r["numberOfCores"], r["memoryInMB"].as_f64().or(r["memoryInMb"].as_f64()).unwrap_or(0.0) / 1024.0))).collect(),
        ("azure", "subnets") => rows().flat_map(|vnet| {
            let (network, group, location) = (text(&vnet, "name").to_owned(), text(&vnet, "resourceGroup").to_owned(), text(&vnet, "location").to_owned());
            vnet["subnets"].as_array().cloned().unwrap_or_default().into_iter().map(move |s| {
                let prefix = s["addressPrefix"].as_str().or_else(|| s["addressPrefixes"][0].as_str()).unwrap_or("");
                item(text(&s, "id"), &format!("{network}/{}", text(&s, "name")), format!("{location} · {prefix} · group {group}"))
            })
        }).filter(|i| region.is_empty() || i["detail"].as_str().is_some_and(|d| d.starts_with(&format!("{region} ")))).collect(),
        ("azure", "securityGroups") => rows().filter(|r| region.is_empty() || text(r, "location") == region).map(|r| item(text(&r, "name"), text(&r, "name"), format!("{} · group {}", text(&r, "location"), text(&r, "resourceGroup")))).collect(),
        ("azure", "resourceGroups") => rows().map(|r| item(text(&r, "name"), text(&r, "name"), text(&r, "location").to_owned())).collect(),
        ("google", "regions") => rows().filter(|r| text(r, "status") == "UP").map(|r| item(text(&r, "name"), text(&r, "name"), text(&r, "region").rsplit('/').next().unwrap_or("").to_owned())).collect(),
        ("google", "images") => rows().map(|r| item(text(&r, "name"), text(&r, "family"), "Project ubuntu-os-cloud (set imageProject to it)")).collect(),
        ("google", "machineTypes") => rows().map(|r| item(text(&r, "name"), text(&r, "name"), format!("{} vCPU · {:.1} GB", r["guestCpus"], r["memoryMb"].as_f64().unwrap_or(0.0) / 1024.0))).collect(),
        ("google", "subnets") => {
            let wanted = region.rsplit_once('-').map(|(r, _)| r).unwrap_or(region);
            rows().filter(|r| text(r, "region").ends_with(&format!("/{wanted}"))).map(|r| item(text(&r, "name"), text(&r, "name"), format!("{} · {}", text(&r, "network").rsplit('/').next().unwrap_or(""), text(&r, "ipCidrRange")))).collect()
        }
        _ => Vec::new(),
    };
    items.retain(|i| i["id"].as_str().is_some_and(word));
    items.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    items.dedup_by(|a, b| a["id"] == b["id"]);
    items
}

/// Public keys in ~/.ssh (never private keys) for Azure and Google Cloud deployments.
fn ssh_keys() -> Vec<Value> {
    let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(std::path::Path::new(&home).join(".ssh")) else { return Vec::new() };
    let mut keys: Vec<Value> = entries.flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "pub"))
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path()).ok()?;
            let mut parts = text.split_whitespace();
            let (kind, key) = (parts.next()?, parts.next()?);
            matches!(kind, "ssh-ed25519" | "ssh-rsa" | "ecdsa-sha2-nistp256").then(|| item(&format!("{kind} {key}"), &e.file_name().to_string_lossy(), parts.next().unwrap_or("").to_owned()))
        }).collect();
    keys.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    keys
}

#[tauri::command]
pub async fn cloud_options(provider: String, account: String, region: Option<String>, kind: String) -> Result<Value, String> {
    let region = region.unwrap_or_default();
    if kind == "sshKeys" {
        return Ok(json!({"provider": provider, "kind": kind, "field": field(&kind), "items": ssh_keys()}));
    }
    if !matches!(provider.as_str(), "aws" | "azure" | "google") { return Err("Choose AWS, Azure or Google Cloud".into()); }
    if !word(&account) { return Err("Give the account: an AWS profile, Azure subscription or Google Cloud project".into()); }
    if !region.is_empty() && !word(&region) { return Err("Invalid region or zone".into()); }
    let mut outputs = Vec::new();
    for args in plan(&provider, &account, &region, &kind)? {
        let output = run_with_timeout(&provider, &args, 120).await.map_err(|e| if e.starts_with("Install") { e } else { format!("{e}. If you are not signed in: `yougori cloud auth --provider {provider} --account {account}`.") })?;
        outputs.push(serde_json::from_str::<Value>(&output).map_err(|_| "The provider CLI did not return JSON".to_string())?);
    }
    let items = items(&provider, &kind, &region, &outputs);
    let mut result = json!({"provider": provider, "kind": kind, "field": field(&kind), "items": items});
    if provider == "google" && kind == "images" { result["imageProject"] = "ubuntu-os-cloud".into(); }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookups_are_read_only_and_scoped_to_the_account() {
        for provider in ["aws", "azure", "google"] {
            for kind in KINDS.iter().filter(|k| **k != "sshKeys") {
                let Ok(plans) = plan(provider, "acct", "eu-west-1", kind) else { continue };
                for args in plans {
                    assert!(args.iter().any(|a| a == "acct" || a == "ubuntu-os-cloud"), "{provider} {kind} {args:?}");
                    assert!(args.iter().any(|a| matches!(a.as_str(), "list" | "list-locations" | "list-sizes" | "describe-from-family" | "get-parameters") || a.starts_with("describe-")), "{args:?}");
                    assert!(!args.iter().any(|a| matches!(a.as_str(), "create" | "delete" | "run-instances" | "terminate-instances")), "{args:?}");
                }
            }
        }
        assert!(plan("aws", "a", "", "machineTypes").unwrap_err().contains("region"));
        assert!(plan("google", "p", "", "subnets").unwrap_err().contains("zone"));
        assert!(plan("azure", "s", "", "keyPairs").is_err());
        assert!(plan("aws", "a", "", "bogus").is_err());
    }
    #[test]
    fn provider_output_becomes_values_the_deploy_request_accepts() {
        let aws = items("aws", "subnets", "", &[json!([{"id":"subnet-1","vpc":"vpc-1","zone":"eu-west-1a","cidr":"10.0.0.0/24","public":true,"tags":[{"Key":"Name","Value":"web"}]}])]);
        assert_eq!((aws[0]["id"].as_str(), aws[0]["name"].as_str()), (Some("subnet-1"), Some("web")));
        assert!(aws[0]["detail"].as_str().unwrap().contains("public IPs"));
        let images = items("aws", "images", "", &[json!([{"name":"/aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id","id":"ami-0abc"}]), json!([])]);
        assert_eq!(images[0]["name"], "Ubuntu 24.04 LTS");
        let vnets = json!([{"name":"net","resourceGroup":"rg","location":"westeurope","subnets":[{"name":"default","id":"/subscriptions/s/resourceGroups/rg/providers/Microsoft.Network/virtualNetworks/net/subnets/default","addressPrefix":"10.1.0.0/24"}]},
            {"name":"far","resourceGroup":"rg","location":"eastus","subnets":[{"name":"x","id":"/subscriptions/s/x"}]}]);
        let azure = items("azure", "subnets", "westeurope", &[vnets]);
        assert_eq!(azure.len(), 1);
        assert_eq!(azure[0]["name"], "net/default");
        let zones = items("google", "subnets", "europe-west1-b", &[json!([{"name":"default","region":"https://x/regions/europe-west1","network":"https://x/networks/default","ipCidrRange":"10.132.0.0/20"},{"name":"other","region":"https://x/regions/us-east1"}])]);
        assert_eq!(zones.len(), 1);
        let unsafe_value = items("aws", "keyPairs", "", &[json!([{"id":"key;rm -rf","type":"rsa"},{"id":"ok-key","type":"ed25519"}])]);
        assert_eq!(unsafe_value.len(), 1);
        assert_eq!(field("machineTypes"), "machineType");
    }
}
