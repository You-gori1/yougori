package main

import (
	"encoding/json"
	"reflect"
	"testing"
)

func TestStartupOnlyChangesProcessArguments(t *testing.T) {
	before := []byte(`{"process":{"args":["old"],"cwd":"/project","env":["KEEP=yes"],"user":{"uid":1000},"terminal":false},"root":{"path":"rootfs"},"mounts":[{"source":"/volume/data","destination":"/data"}],"linux":{"devices":[{"path":"/dev/nvidia0"}],"resources":{"memory":{"limit":6442450944}}},"annotations":{"custom":"keep"}}`)
	args := []string{"entrypoint.sh", "/bin/sh", "-lc", "npm run build && NODE_ENV=production node server/dist/index.js"}
	updated, err := startupSpec(before, args)
	if err != nil {
		t.Fatal(err)
	}
	var original, changed map[string]any
	if err = json.Unmarshal(before, &original); err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal(updated, &changed); err != nil {
		t.Fatal(err)
	}
	expectedArgs := make([]any, len(args))
	for i, value := range args {
		expectedArgs[i] = value
	}
	original["process"].(map[string]any)["args"] = expectedArgs
	if !reflect.DeepEqual(original, changed) {
		t.Fatal("startup edit changed fields beyond process arguments")
	}
}

func TestStartupRejectsMissingProcessAndEmptyDefaults(t *testing.T) {
	for _, spec := range []string{`null`, `{}`, `{"process":null}`, `invalid`} {
		if _, err := startupSpec([]byte(spec), []string{"sh"}); err == nil {
			t.Fatal("accepted missing process")
		}
	}
	if _, err := startupSpec([]byte(`{"process":{"args":["old"]}}`), nil); err == nil {
		t.Fatal("accepted an empty command")
	}
}
