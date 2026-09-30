package main

import (
	"strings"
	"testing"
)

func TestContainerFailureDetails(t *testing.T) {
	if failureDescription(containerState{Running: true}, "secret") != "" {
		t.Fatal("running container must not report a failure")
	}
	if failureDescription(containerState{Running: false, Paused: true, ExitCode: 0}, "old failed startup") != "" {
		t.Fatal("a snapshot pause must not report an exit or stale logs")
	}
	for _, status := range []string{"pausing", "paused", "restarting"} {
		if failureDescription(containerState{Status: status}, "old failed startup") != "" {
			t.Fatalf("transitional state %q was reported as an exit", status)
		}
	}
	for _, item := range []struct {
		state containerState
		text  string
	}{
		{containerState{ExitCode: 137, OOMKilled: true}, "memory"},
		{containerState{ExitCode: 132}, "CPU instruction"},
		{containerState{ExitCode: 0}, "startup command finished"},
	} {
		if !strings.Contains(failureDescription(item.state, "example log"), item.text) {
			t.Fatal(item.text)
		}
	}
}

func TestContainerLogTailIsBounded(t *testing.T) {
	var b logTail
	input := []byte(strings.Repeat("a", 20000))
	n, err := b.Write(input)
	if err != nil || n != len(input) || len(b.data) != 8192 {
		t.Fatal("unbounded log")
	}
	b.Write([]byte("last"))
	if len(b.data) != 8192 || !strings.HasSuffix(string(b.data), "last") {
		t.Fatal("missing tail")
	}
}
