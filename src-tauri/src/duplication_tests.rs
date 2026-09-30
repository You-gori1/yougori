use super::*;

fn location(provider: &str) -> Location {
    Location {
        provider: provider.into(),
        account: "account".into(),
        region: "region".into(),
        instance: "instance-123".into(),
        resource_group: "group".into(),
        bucket: "private-transfer".into(),
    }
}

#[test]
fn inspection_always_scopes_the_account_and_never_mutates_a_source() {
    for provider in ["aws", "azure", "google"] {
        let source = location(provider);
        let args = inspection_plan(&source);
        assert!(args.iter().any(|v| v == "account"));
        assert!(args.iter().any(|v| v == "instance-123"));
        assert!(!args
            .iter()
            .any(|v| ["stop", "delete", "start", "deallocate"].contains(&v.as_str())));
        assert!(validate_location(&source).is_ok());
        let mut invalid = source;
        invalid.account = "--another-account".into();
        assert!(validate_location(&invalid).is_err());
    }
}

#[test]
fn incomplete_or_running_machines_cannot_be_silently_copied() {
    let mut aws = json!({"Architecture":"x86_64","State":{"Name":"stopped"},"BlockDeviceMappings":[{"Ebs":{"VolumeId":"vol-123"}}]});
    assert_eq!(validate_machine(&location("aws"), &aws).unwrap(), "vol-123");
    aws["State"]["Name"] = json!("running");
    assert!(validate_machine(&location("aws"), &aws)
        .unwrap_err()
        .contains("Stop the source"));
    aws["State"]["Name"] = json!("stopped");
    aws["BlockDeviceMappings"]
        .as_array_mut()
        .unwrap()
        .push(json!({"Ebs":{"VolumeId":"vol-data"}}));
    assert!(validate_machine(&location("aws"), &aws)
        .unwrap_err()
        .contains("additional disks"));
    let azure = json!({"powerState":"VM deallocated","storageProfile":{"osDisk":{"osType":"Linux","managedDisk":{"id":"/subscriptions/s/disks/d"}},"dataDisks":[]}});
    assert!(validate_machine(&location("azure"), &azure).is_ok());
    let mut google = json!({"status":"TERMINATED","disks":[{"boot":true,"source":"https://compute.googleapis.com/projects/p/disks/d"}]});
    assert!(validate_machine(&location("google"), &google).is_ok());
    google["disks"][0]["boot"] = json!(false);
    assert!(validate_machine(&location("google"), &google).is_err());
}

#[test]
fn transfer_buckets_cannot_be_urls_paths_or_options() {
    assert!(bucket("my-private-bucket"));
    for invalid in [
        "s3://bucket",
        "bucket/path",
        "--bucket",
        ".bucket",
        "bucket.",
        "bucket..name",
        "Bucket",
        "a",
    ] {
        assert!(!bucket(invalid), "{invalid}");
    }
}

#[test]
fn native_clones_do_not_cross_an_account_or_region_boundary() {
    let source = location("aws");
    assert!(same_location(&source, &source));
    for changed in [
        Location {
            account: "other".into(),
            ..source.clone()
        },
        Location {
            region: "other".into(),
            ..source.clone()
        },
        location("azure"),
    ] {
        assert!(!same_location(&source, &changed));
    }
}

#[tokio::test]
async fn completed_steps_are_not_reissued_and_unknown_creation_is_not_retried() {
    let directory = tempfile::tempdir().unwrap();
    let store = PlatformStore::load(directory.path().join("state.json")).unwrap();
    let request: Request = serde_json::from_value(json!({"operationId":uuid::Uuid::new_v4().to_string(),"environmentId":"source","name":"copy","destination":"cloud","source":null,"target":null,"storageDrive":null,"reviewed":true})).unwrap();
    let mut op = Operation {
        id: request.operation_id.clone(),
        store: &store,
        job: Job {
            request,
            environment_id: "destination".into(),
            status: "running".into(),
            phase: "test".into(),
            error: None,
            resources: vec![],
            completed: BTreeMap::from([("done".into(), json!({"id":"existing"}))]),
            pending: Some("uncertain".into()),
        },
    };
    assert_eq!(
        op.step("done", "never-execute", vec![], None, false)
            .await
            .unwrap(),
        json!({"id":"existing"})
    );
    assert!(op
        .step("uncertain", "never-execute", vec![], None, false)
        .await
        .unwrap_err()
        .contains("will not create a second resource"));
    op.save().unwrap();
    let reloaded = PlatformStore::load(directory.path().join("state.json"))
        .unwrap()
        .snapshot()
        .unwrap();
    assert_eq!(reloaded.duplication_jobs[&op.id].status, "interrupted");
    assert_eq!(
        reloaded.duplication_jobs[&op.id].pending.as_deref(),
        Some("uncertain")
    );
}

#[tokio::test]
async fn all_three_providers_resume_a_lost_create_response_without_creating_another_vm() {
    use std::{cell::RefCell, rc::Rc};
    for provider in ["aws", "azure", "google"] {
        let temp = tempfile::tempdir().unwrap();
        let store = PlatformStore::load(temp.path().join("state.json")).unwrap();
        let runtime =
            RuntimeManager::new(Path::new(env!("CARGO_MANIFEST_DIR")), temp.path()).unwrap();
        let source = location(provider);
        let environment: Environment = serde_json::from_value(json!({"id":"source","name":"Original","kind":"cloud","provider":"cloudSsh","status":"stopped","runtime":"original@host","description":"source","createdAt":"2026-01-01","cpuUsage":0,"memoryUsageGb":0,"storageDeltaGb":0,"networkRxMbps":0,"resourcePolicy":{"cpu":{"min":0,"preferred":0,"max":0,"current":0},"memoryGb":{"min":0,"preferred":0,"max":0,"current":0},"priority":"normal","dynamic":false}})).unwrap();
        store
            .mutate(|state| {
                state.environments.push(environment.clone());
                state.cloud_deployments.insert(
                    "source".into(),
                    Deployment {
                        provider: provider.into(),
                        account: source.account.clone(),
                        region: source.region.clone(),
                        resource_group: source.resource_group.clone(),
                        name: source.instance.clone(),
                        resource_id: source.instance.clone(),
                        state: "Stopped".into(),
                        address: "host".into(),
                        username: "ubuntu".into(),
                        request_id: "original".into(),
                        last_error: None,
                    },
                );
                Ok(())
            })
            .unwrap();
        let target: DeployRequest = serde_json::from_value(json!({"provider":provider,"account":"account","region":"region","resourceGroup":"group","name":"copy","image":"ignored","machineType":"small","subnet":"existing-subnet","securityGroup":"existing-security","keyPair":"keypair","sshPublicKey":"ssh-ed25519 AAAA","imageProject":"account","username":"ubuntu"})).unwrap();
        let request = Request {
            local_files: None,
            operation_id: uuid::Uuid::new_v4().to_string(),
            environment_id: "source".into(),
            name: "Copy".into(),
            destination: "cloud".into(),
            source: Some(source),
            target: Some(target),
            target_bucket: String::new(),
            storage_drive: None,
            reviewed: true,
        };
        let vm_name = key(&request.operation_id);
        let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(vec![]));
        let recorded = calls.clone();
        let mut fail_create = true;
        let owner = request.operation_id.clone();
        let handler: Box<dyn FnMut(&str, &[String]) -> Result<String, String>> = Box::new(
            move |service, args| {
                assert_eq!(service, provider);
                recorded.borrow_mut().push(args.to_vec());
                assert!(!args.iter().any(
                    |arg| ["stop", "deallocate", "terminate-instances"].contains(&arg.as_str())
                ));
                let is = |prefix: &[&str]| {
                    args.iter()
                        .take(prefix.len())
                        .map(String::as_str)
                        .eq(prefix.iter().copied())
                };
                let destination = json!({"tags":{"YougoriCopy":owner},"InstanceId":"i-new","PrivateIpAddress":"10.0.0.2","id":"/new-vm","selfLink":"https://compute.googleapis.com/new-vm","privateIpAddress":"10.0.0.2","networkInterfaces":[{"networkIP":"10.0.0.2"}]});
                if is(&["ec2", "run-instances"])
                    || is(&["vm", "create"])
                    || is(&["compute", "instances", "create"])
                {
                    assert!(args
                        .iter()
                        .any(|arg| arg.contains(&vm_name) || arg.contains("YougoriId")));
                    if fail_create {
                        fail_create = false;
                        return Err(
                            "The provider accepted creation but the response was lost".into()
                        );
                    }
                    panic!("A resumed copy must never issue another VM create");
                }
                let value = if is(&["ec2", "describe-instances"]) {
                    if args.iter().any(|arg| arg == "--filters") {
                        if fail_create {
                            json!({"Reservations":[]})
                        } else {
                            json!({"Reservations":[{"Instances":[destination]}]})
                        }
                    } else {
                        json!({"Reservations":[{"Instances":[{"Architecture":"x86_64","CurrentInstanceBootMode":"uefi","State":{"Name":"stopped"},"BlockDeviceMappings":[{"Ebs":{"VolumeId":"vol-source"}}]}]}]})
                    }
                } else if is(&["ec2", "describe-images"]) {
                    json!({"Images":[]})
                } else if is(&["snapshot", "show"]) || is(&["compute", "images", "describe"]) {
                    return Err("Resource not found".into());
                } else if is(&["ec2", "create-image"]) {
                    json!({"ImageId":"ami-owned-copy"})
                } else if is(&["ec2", "wait"]) {
                    return Ok(String::new());
                } else if is(&["vm", "show"]) {
                    if args.iter().any(|arg| arg == &vm_name) {
                        if fail_create {
                            return Err("Resource not found".into());
                        }
                        destination
                    } else {
                        json!({"powerState":"VM deallocated","storageProfile":{"osDisk":{"osType":"Linux","managedDisk":{"id":"/source-disk"}},"dataDisks":[]}})
                    }
                } else if is(&["disk", "show"]) {
                    if args.iter().any(|arg| arg == "--name") {
                        return Err("Resource not found".into());
                    }
                    json!({"hyperVGeneration":"V2","supportedCapabilities":{"architecture":"x64"}})
                } else if is(&["snapshot", "create"]) {
                    json!({"id":"/owned-copy-snapshot"})
                } else if is(&["disk", "create"]) {
                    json!({"id":"/owned-copy-disk"})
                } else if is(&["compute", "instances", "describe"]) {
                    if args.iter().any(|arg| arg == &vm_name) {
                        if fail_create {
                            return Err("Resource not found".into());
                        }
                        destination
                    } else {
                        json!({"status":"TERMINATED","disks":[{"boot":true,"source":"https://compute.googleapis.com/disks/source"}]})
                    }
                } else if is(&["compute", "disks", "describe"]) {
                    json!({"architecture":"X86_64","guestOsFeatures":[{"type":"UEFI_COMPATIBLE"}]})
                } else if is(&["compute", "images", "create"]) {
                    json!([{"name":vm_name}])
                } else if is(&["ec2", "deregister-image"]) {
                    json!({"Return":true,"DeleteSnapshotResults":[{"SnapshotId":"snapshot-copy","ReturnCode":"success"}]})
                } else if is(&["snapshot", "delete"]) || is(&["compute", "images", "delete"]) {
                    Value::Null
                } else {
                    panic!("Unexpected {service} command: {args:?}")
                };
                Ok(value.to_string())
            },
        );
        TEST_CLI
            .scope(RefCell::new(handler), async {
                let job = Job {
                    request: request.clone(),
                    environment_id: "destination".into(),
                    status: "running".into(),
                    phase: "test".into(),
                    error: None,
                    resources: vec![],
                    completed: BTreeMap::new(),
                    pending: None,
                };
                let mut op = Operation {
                    id: request.operation_id.clone(),
                    store: &store,
                    job,
                };
                op.save().unwrap();
                assert!(run_copy(&mut op, &environment, &runtime)
                    .await
                    .unwrap_err()
                    .contains("response was lost"));
                assert_eq!(op.job.pending.as_deref(), Some("destination-vm"));
                let reloaded = PlatformStore::load(temp.path().join("state.json")).unwrap();
                let job =
                    reloaded.snapshot().unwrap().duplication_jobs[&request.operation_id].clone();
                let mut resumed = Operation {
                    id: request.operation_id.clone(),
                    store: &reloaded,
                    job,
                };
                run_copy(&mut resumed, &environment, &runtime)
                    .await
                    .unwrap();
                let state = reloaded.snapshot().unwrap();
                assert_eq!(state.environments.len(), 2);
                assert_eq!(
                    serde_json::to_value(&state.environments[0]).unwrap(),
                    serde_json::to_value(&environment).unwrap()
                );
                assert_eq!(state.cloud_deployments["destination"].address, "10.0.0.2");
                assert_eq!(
                    state
                        .environments
                        .iter()
                        .find(|e| e.id == "destination")
                        .unwrap()
                        .status,
                    EnvironmentStatus::Stopped
                );
                resumed.job.status = "complete".into();
                cleanup_transfer(&mut resumed, &runtime).await.unwrap();
                assert!(resumed.job.completed.contains_key("cleanup"));
            })
            .await;
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .filter(|args| args.starts_with(&words(&["ec2", "run-instances"]))
                    || args.starts_with(&words(&["vm", "create"]))
                    || args.starts_with(&words(&["compute", "instances", "create"])))
                .count(),
            1
        );
        assert!(!calls
            .iter()
            .any(|args| args.starts_with(&words(&["disk", "delete"]))
                || args.starts_with(&words(&["vm", "delete"]))));
    }
}

#[tokio::test]
async fn same_name_foreign_cloud_resources_are_never_adopted_or_overwritten() {
    use std::cell::RefCell;
    for provider in ["aws", "azure", "google"] {
        let directory = tempfile::tempdir().unwrap();
        let store = PlatformStore::load(directory.path().join("state.json")).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let request: Request = serde_json::from_value(json!({"operationId":id,"environmentId":"source","name":"copy","destination":"cloud","source":null,"target":null,"storageDrive":null,"reviewed":true})).unwrap();
        let handler: Box<dyn FnMut(&str, &[String]) -> Result<String, String>> =
            Box::new(|_, args| {
                assert_eq!(
                    args,
                    &["inspect".to_string()],
                    "a foreign resource must never be mutated"
                );
                Ok(json!({"id":"foreign","tags":{"YougoriCopy":"another-operation"}}).to_string())
            });
        TEST_CLI
            .scope(RefCell::new(handler), async {
                let mut op = Operation {
                    id,
                    store: &store,
                    job: Job {
                        request,
                        environment_id: "destination".into(),
                        status: "running".into(),
                        phase: "test".into(),
                        error: None,
                        resources: vec![],
                        completed: BTreeMap::new(),
                        pending: None,
                    },
                };
                let error = op
                    .step(
                        "destination-vm",
                        provider,
                        words(&["create"]),
                        Some(words(&["inspect"])),
                        false,
                    )
                    .await
                    .unwrap_err();
                assert!(error.contains("not tagged as belonging to this copy"));
                assert!(op.job.completed.is_empty());
                assert!(op.job.pending.is_none());
            })
            .await;
    }
}
