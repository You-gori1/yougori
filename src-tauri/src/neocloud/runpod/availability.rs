//! Request-specific stock. runpodctl's GPU list only aggregates data-center stock;
//! it cannot check Community public IPs, GPU count or disk requirements.
use super::{accounts, number, stock, PodRequest, PROVIDER};
use serde_json::{json, Value};
use std::time::Duration;

const QUERY: &str = r#"
query YougoriGpuOffers($filter: GpuTypeFilter, $secure: GpuLowestPriceInput!, $community: GpuLowestPriceInput!) {
  gpuTypes(input: $filter) {
    id displayName memoryInGb secureCloud communityCloud securePrice communityPrice
    secure: lowestPrice(input: $secure) { stockStatus uninterruptablePrice availableGpuCounts }
    community: lowestPrice(input: $community) { stockStatus uninterruptablePrice availableGpuCounts }
  }
}"#;

fn variables(gpu_id: Option<&str>, input: Value) -> Value {
    let mut secure = input.clone();
    secure["secureCloud"] = json!(true);
    // Secure pods always have public IPs; match runpodctl's creation request.
    secure["supportPublicIp"] = json!(false);
    let mut community = input;
    community["secureCloud"] = json!(false);
    json!({"filter":gpu_id.map(|id| json!({"id":id})),"secure":secure,"community":community})
}

pub(super) async fn read(gpu_id: Option<&str>, input: Value) -> Result<Value, String> {
    let count = input["gpuCount"].as_u64().unwrap_or(1);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not set up the RunPod stock check")?;
    // Inventory is public. Use the saved account when present; CLI-managed
    // accounts can still read it without copying credentials from their config.
    let mut request = client
        .post("https://api.runpod.io/graphql")
        .json(&json!({"query":QUERY,"variables":variables(gpu_id, input)}));
    if let Some(key) = accounts::saved_key(PROVIDER)? {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Could not check RunPod stock. Refresh and try again.")?;
    if !response.status().is_success() {
        return Err(format!(
            "RunPod stock check failed (HTTP {}). Refresh and try again.",
            response.status().as_u16()
        ));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "RunPod returned an unreadable stock response")?;
    parse(&value, count)
}

fn parse(value: &Value, count: u64) -> Result<Value, String> {
    // Never turn a failed/partial lookup into 'out of stock'.
    if value["errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty())
    {
        return Err("RunPod could not check GPU stock. Refresh and try again.".into());
    }
    let rows = value["data"]["gpuTypes"]
        .as_array()
        .ok_or("RunPod returned no GPU stock data")?;
    let gpus: Vec<Value> = rows
        .iter()
        .map(|row| {
            json!({
                "id":row["id"],"name":row["displayName"],"vramGb":row["memoryInGb"],
                "secureOffer":offer(row, "secure", count),
                "communityOffer":offer(row, "community", count),
            })
        })
        .collect();
    Ok(json!({"gpus":gpus,"checkedAt":chrono::Utc::now().to_rfc3339()}))
}

fn offer(row: &Value, cloud: &str, count: u64) -> Value {
    let enabled = row[format!("{cloud}Cloud")] == true;
    let live = &row[cloud];
    let price = number(&live["uninterruptablePrice"]).filter(|p| *p > 0.0);
    let available = enabled
        && price.is_some()
        && stock(&live["stockStatus"]) != "none"
        && live["availableGpuCounts"]
            .as_array()
            .is_none_or(|counts| counts.iter().any(|n| n.as_u64() == Some(count)));
    json!({"available":available,"stock":stock(&live["stockStatus"]),
        "price":if enabled { price.or_else(|| number(&row[format!("{cloud}Price")]).filter(|p| *p > 0.0)) } else { None }})
}

fn pod_input(r: &PodRequest) -> Value {
    let mut input = json!({"gpuCount":r.gpu_count,
        "minDisk":r.container_disk_gb + if r.network_volume_id.is_empty() { r.volume_gb } else { 0 },
        "supportPublicIp":r.public_ip});
    if !r.location.is_empty() {
        input["dataCenterId"] = json!(r.location);
    }
    if !r.min_cuda.is_empty() {
        input["minCudaVersion"] = json!(r.min_cuda);
    }
    if !r.compliance.is_empty() {
        input["compliance"] = json!(r.compliance);
    }
    if r.global_networking {
        input["globalNetwork"] = json!(true);
    }
    input
}

pub(super) async fn quote(r: &PodRequest) -> Result<f64, String> {
    let inventory = read(Some(&r.gpu_id), pod_input(r)).await?;
    reviewed_price(&inventory, r)
}

fn reviewed_price(inventory: &Value, r: &PodRequest) -> Result<f64, String> {
    let row = inventory["gpus"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|g| g["id"] == r.gpu_id)
        .ok_or("Neocloud no longer offers this GPU. Choose another one.")?;
    let selected = &row[format!("{}Offer", r.cloud)];
    let name = row["name"].as_str().unwrap_or(&r.gpu_id);
    if selected["available"] != true {
        return Err(format!(
            "{name} is out of stock right now. Choose another GPU or try again in a few minutes."
        ));
    }
    let total = number(&selected["price"]).ok_or("RunPod returned no current GPU price")?
        * f64::from(r.gpu_count);
    let approved = r.max_hourly_usd.unwrap_or(0.0);
    if total > approved + 1e-9 {
        return Err(format!("The price of {name} went up to ${total:.2}/hr since you reviewed it (${approved:.2}/hr). Review it again."));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_stock_is_independent_and_a_price_is_not_stock() {
        let row = json!({"id":"gpu","secureCloud":true,"communityCloud":true,"securePrice":0.5,"communityPrice":0.2,
            "secure":{"stockStatus":"Low","uninterruptablePrice":0.5},
            "community":{"stockStatus":null,"uninterruptablePrice":null}});
        assert_eq!(offer(&row, "secure", 1)["available"], true);
        assert_eq!(offer(&row, "community", 1)["available"], false);
        assert_eq!(offer(&row, "community", 1)["price"], 0.2);
        let mut limited = row;
        limited["secure"]["availableGpuCounts"] = json!([2, 4]);
        assert_eq!(offer(&limited, "secure", 1)["available"], false);
        assert_eq!(offer(&limited, "secure", 2)["available"], true);
        assert!(parse(
            &json!({"errors":[{"message":"failed"}],"data":{"gpuTypes":[]}}),
            1
        )
        .is_err());
        assert!(parse(&json!({}), 1).is_err());
    }

    #[test]
    fn preflight_matches_creation_constraints_and_reviewed_price() {
        let mut r: PodRequest =
            serde_json::from_value(json!({"name":"test","compute":"gpu","gpuId":"gpu",
            "cloud":"community","gpuCount":2,"containerDiskGb":40,"volumeGb":20,"publicIp":true,
            "image":"test","maxHourlyUsd":0.4,"location":"EU-RO-1","minCuda":"12.8"}))
            .unwrap();
        let vars = variables(Some(&r.gpu_id), pod_input(&r));
        assert_eq!(vars["community"]["supportPublicIp"], true);
        assert_eq!(vars["community"]["secureCloud"], false);
        assert_eq!(vars["secure"]["supportPublicIp"], false);
        assert_eq!(vars["secure"]["secureCloud"], true);
        assert_eq!(vars["community"]["minDisk"], 60);
        assert_eq!(vars["community"]["gpuCount"], 2);
        assert_eq!(vars["community"]["dataCenterId"], "EU-RO-1");
        assert_eq!(vars["community"]["minCudaVersion"], "12.8");
        let inventory = json!({"gpus":[{"id":"gpu","communityOffer":{"available":true,"price":0.2},
            "secureOffer":{"available":false,"price":0.5}}]});
        assert_eq!(reviewed_price(&inventory, &r).unwrap(), 0.4);
        r.max_hourly_usd = Some(0.3);
        assert!(reviewed_price(&inventory, &r)
            .unwrap_err()
            .contains("went up"));
        r.cloud = "secure".into();
        assert!(reviewed_price(&inventory, &r)
            .unwrap_err()
            .contains("out of stock"));
    }
}
