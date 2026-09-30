package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestInternetPluginRejectsUnsafeIdentifiers(t *testing.T) {
	for _, id := range []string{"", "../other", "a/b", "a\nother"} {
		if err := internetPlugin(context.Background(), "ADD", id, 42); err == nil {
			t.Fatalf("accepted unsafe ID %q", id)
		}
	}
	if err := internetPlugin(context.Background(), "ADD", "safe", 1); err == nil {
		t.Fatal("accepted appliance network namespace")
	}
}

func TestInternetCNIUsesDedicatedIPv4Uplink(t *testing.T) {
	var config struct {
		Name       string `json:"name"`
		Bridge     string `json:"bridge"`
		Gateway    bool   `json:"isGateway"`
		Masquerade bool   `json:"ipMasq"`
		IPAM       struct {
			Subnet string `json:"subnet"`
		} `json:"ipam"`
	}
	if err := json.Unmarshal([]byte(internetCNI), &config); err != nil {
		t.Fatal(err)
	}
	if config.Name != "opendock-internet" || config.Bridge != "odinet0" || !config.Gateway || !config.Masquerade || config.IPAM.Subnet != "10.90.0.0/16" {
		t.Fatalf("unexpected uplink config: %+v", config)
	}
}

func TestLiveInternetChangesOnlyTheApplianceCable(t *testing.T) {
	directory := t.TempDir()
	log := filepath.Join(directory, "commands")
	script := `#!/bin/sh
printf '%s %s\n' "${0##*/}" "$*" >> "$OD_TEST_COMMANDS"
case "${0##*/}" in
 nerdctl) printf '42\n' ;;
 nsenter)
  case "$*" in
   *"ip -j link show"*)
    if [ "$OD_TEST_EMPTY" = 1 ]; then printf '[{"ifindex":1,"ifname":"lo"}]';
    else printf '[{"ifindex":1,"ifname":"lo"},{"ifindex":2,"ifname":"eth0","link_index":77}]'; fi ;;
  esac ;;
 ip)
  case "$*" in
   "-j link show") printf '[{"ifindex":3,"ifname":"eth0"},{"ifindex":77,"ifname":"veth-managed"}]' ;;
  esac ;;
esac
`
	for _, name := range []string{"nerdctl", "nsenter", "ip"} {
		if err := os.WriteFile(filepath.Join(directory, name), []byte(script), 0755); err != nil {
			t.Fatal(err)
		}
	}
	t.Setenv("PATH", directory+":"+os.Getenv("PATH"))
	t.Setenv("OD_TEST_COMMANDS", log)
	for _, enabled := range []bool{false, true, false} {
		if err := setContainerInternet(context.Background(), "env-test", enabled); err != nil {
			t.Fatal(err)
		}
	}
	contents, err := os.ReadFile(log)
	if err != nil {
		t.Fatal(err)
	}
	commands := string(contents)
	if strings.Count(commands, "ip link set dev veth-managed down\n") != 2 || !strings.Contains(commands, "ip link set dev veth-managed up\n") {
		t.Fatalf("wrong cable changes: %s", commands)
	}
	for _, forbidden := range []string{"nerdctl --namespace opendock stop", "nerdctl --namespace opendock restart", "ip link set dev eth0", "ip link del"} {
		if strings.Contains(commands, forbidden) {
			t.Fatalf("unexpected destructive command: %s", forbidden)
		}
	}
	t.Setenv("OD_TEST_EMPTY", "1")
	if err := setContainerInternet(context.Background(), "env-test", false); err != nil {
		t.Fatal(err)
	}
}
