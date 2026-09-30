package main

import (
	"strings"
	"testing"
)

func TestWorkloadOptionsKeepEntrypointAndLiteralArguments(t *testing.T) {
	argv := []string{"--flag", "a; touch /never"}
	o := workloadOptions{Environment: map[string]string{"PASSWORD": "a b\nquote'"}, Args: &argv, Volumes: []workloadVolume{{Source: "project-db", Target: "/var/lib/db"}}}
	args, err := appendWorkload([]string{"create"}, "postgres:17", "", o)
	if err != nil {
		t.Fatal(err)
	}
	if args[len(args)-1] != argv[1] || strings.Contains(strings.Join(args, "|"), "/bin/sh") {
		t.Fatal(args)
	}
	for _, target := range []string{"/", "/proc/x", "/etc/../root", "/dev"} {
		o.Volumes[0].Target = target
		if _, err = o.arguments(); err == nil {
			t.Fatal("accepted", target)
		}
	}
}
func TestExplicitShellCommandOverridesImageEntrypoint(t *testing.T) {
	a, e := appendWorkload(nil, "image", "echo hi", workloadOptions{})
	if e != nil || strings.Join(a, "|") != "--entrypoint|/bin/sh|image|-lc|echo hi" {
		t.Fatal(a, e)
	}
}
