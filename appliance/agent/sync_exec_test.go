package main

import (
	"encoding/json"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestSynchronousContainerExecPreservesGuestExitAndOutput(t *testing.T) {
	root := t.TempDir()
	// Reproduce the older provider's real exit mapping observed on the Pi.
	adapter := "#!/bin/sh\nshift 4\n\"$@\"\nstatus=$?\nif test \"$status\" != 0; then printf 'provider fatal: guest exit %s\\n' \"$status\" >&2; exit 1; fi\n"
	if err := os.WriteFile(filepath.Join(root, "nerdctl"), []byte(adapter), 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", root+":"+os.Getenv("PATH"))
	for _, code := range []int{0, 7, 17, 127, 255} {
		t.Run(fmt.Sprint(code), func(t *testing.T) {
			request, _ := json.Marshal(execRequest{ID: "env-fixture", Command: fmt.Sprintf("printf 'literal 日本語 | $(not-a-command)'; printf 'stderr marker' >&2; exit %d", code)})
			reply := httptest.NewRecorder()
			(&server{}).execute(reply, httptest.NewRequest("POST", "/v1/containers/exec", strings.NewReader(string(request))))
			var output commandOutput
			if reply.Code != 200 || json.Unmarshal(reply.Body.Bytes(), &output) != nil {
				t.Fatal(reply.Code, reply.Body.String())
			}
			if output.ExitCode != code || output.Stdout != "literal 日本語 | $(not-a-command)" || output.Stderr != "stderr marker" {
				t.Fatal("guest status or output was altered", output)
			}
		})
	}
}

func TestSynchronousContainerExecDoesNotRetryUnknownOutcome(t *testing.T) {
	root := t.TempDir()
	marker := filepath.Join(root, "applied")
	adapter := "#!/bin/sh\nshift 4\ncase \"$3\" in 'test -f '*) rm -f \"$5\"; printf 'bad'; exit 0;; esac\nexec \"$@\"\n"
	if err := os.WriteFile(filepath.Join(root, "nerdctl"), []byte(adapter), 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", root+":"+os.Getenv("PATH"))
	request, _ := json.Marshal(execRequest{ID: "env-fixture", Command: "printf 'applied once\\n' >> '" + strings.ReplaceAll(marker, "'", "'\"'\"'") + "'"})
	reply := httptest.NewRecorder()
	(&server{}).execute(reply, httptest.NewRequest("POST", "/v1/containers/exec", strings.NewReader(string(request))))
	if reply.Code != 500 || !strings.Contains(reply.Body.String(), "outcome unknown") {
		t.Fatal(reply.Code, reply.Body.String())
	}
	content, err := os.ReadFile(marker)
	if err != nil || string(content) != "applied once\n" {
		t.Fatal("an ambiguous command was retried", string(content), err)
	}
}
