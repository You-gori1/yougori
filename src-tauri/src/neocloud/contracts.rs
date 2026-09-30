use super::*;

fn request(provider: &str) -> CreateRequest {
    serde_json::from_value(json!({"provider":provider,"product":"gpu","name":"training-1","image":"base","offer":"h100","location":"US","diskGb":100,"platform":"project-1","subnetId":"ssh-1","cpuCores":8,"memoryGb":32,"gpuCount":1,"maxHourlyUsd":2.0})).unwrap()
}

#[test]
fn every_new_provider_has_a_complete_noninteractive_creation_plan() {
    let prime = request("prime");
    validate(&prime).unwrap();
    let args = create_args(&prime);
    for (flag, value) in [
        ("--vcpus", "8"),
        ("--memory", "32"),
        ("--disk-size", "100"),
        ("--image", "base"),
    ] {
        assert!(args.windows(2).any(|pair| pair == [flag, value]));
    }
    assert!(args.contains(&"--yes".into()));
    let mut thunder = request("thunder");
    thunder.gpu_count = 2;
    assert!(validate(&thunder).is_err());
    thunder.disk_gb = 200;
    validate(&thunder).unwrap();
    assert!(create_args(&thunder)
        .windows(2)
        .any(|pair| pair == ["--num-gpus", "2"]));
    assert!(create_args(&thunder)
        .windows(2)
        .any(|pair| pair == ["--vcpus", "8"]));
    let latitude = request("latitude");
    validate(&latitude).unwrap();
    assert!(create_args(&latitude)
        .windows(2)
        .any(|pair| pair == ["--ssh_keys", "ssh-1"]));
    let mut e2e = request("e2e");
    e2e.location = "123:Mumbai".into();
    e2e.firewall = "42".into();
    e2e.ssh_public_key = super::tests::test_ssh_key();
    validate(&e2e).unwrap();
    let args = create_args(&e2e);
    assert_eq!(&args[..4], ["--project_id", "123", "--location", "Mumbai"]);
    assert!(args.windows(2).any(|pair| pair == ["--region", "mumbai"]));
    assert!(!args.contains(&"--auto".into()));
}

#[test]
fn paid_requests_reject_incomplete_or_injected_configuration() {
    let mut prime = request("prime");
    prime.memory_gb = 0;
    assert!(validate(&prime).is_err());
    let mut latitude = request("latitude");
    latitude.subnet_id.clear();
    assert!(validate(&latitude).is_err());
    let mut runpod = request("runpod");
    runpod.location = "US; malicious".into();
    assert!(validate(&runpod).is_err());
    runpod.location = "US-CA-2".into();
    assert!(create_args(&runpod)
        .windows(2)
        .any(|pair| pair == ["--data-center-ids", "US-CA-2"]));
    runpod.max_hourly_usd = None;
    assert!(validate(&runpod).is_err());
    runpod.max_hourly_usd = Some(2.0);
    runpod.gpu_count = 2;
    assert!(validate(&runpod).is_err());
}

#[test]
fn new_provider_response_contracts_preserve_ids_and_reject_errors() {
    assert_eq!(
        identifier(
            "latitude",
            &json!([{"id":"sv-1","attributes":{"hostname":"training-1"}}])
        ),
        Some("sv-1".into())
    );
    assert_eq!(
        provider_commands::resource(
            "latitude",
            "sv-1",
            json!([{"id":"sv-1","attributes":{"status":"on"}}])
        )
        .unwrap()["status"],
        "on"
    );
    assert_eq!(
        identifier("thunder", &json!({"uuid":"uuid-1","identifier":0})),
        Some("0".into())
    );
    assert_eq!(
        provider_commands::resource("thunder", "0", json!([{"id":"0","status":"RUNNING"}]))
            .unwrap()["status"],
        "RUNNING"
    );
    assert!(provider_commands::resource("thunder", "99", json!([])).is_err());
    assert!(
        parse_output("latitude", "✗ Error: [GET /servers][404] not found")
            .unwrap_err()
            .contains("404")
    );
    assert!(parse_output("latitude", "✗ Error: Invalid account").is_err());
    assert_eq!(
        parse_output("e2e", "Creating\n{\"code\":200,\"data\":{\"id\":12}}").unwrap()["data"]["id"],
        12
    );
    for (provider, output) in [
        ("e2e", r#"{"code":403,"data":null}"#),
        ("latitude", r#"{"errors":[{"detail":"denied"}]}"#),
        ("thunder", r#"{"error":"denied"}"#),
    ] {
        assert!(parse_output(provider, output).is_err());
    }
}

#[test]
fn deletion_only_providers_cannot_be_accidentally_terminated_by_stop() {
    for provider in ["prime", "thunder"] {
        let deployment: Deployment = serde_json::from_value(json!({"provider":provider,"product":"gpu","name":"training-1","resourceId":"1","state":"Running","image":"base","offer":"h100","address":"","requestId":"test","lastError":null})).unwrap();
        assert!(action_args(&deployment, "stop").is_err());
        assert!(action_args(&deployment, "start").is_err());
        let delete = action_args(&deployment, "delete").unwrap();
        assert!(delete.contains(&"--yes".into()));
    }
}
