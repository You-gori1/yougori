package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestInteractiveTerminalRestoresColourWithoutChangingOtherEnvironment(t *testing.T) {
	bin := t.TempDir()
	// An executable shell stand-in inspects the environment and login arguments
	// passed by the real bootstrap, without loading a personal shell profile.
	probe := "#!/bin/sh\nprintf '%s\\n' \"$TERM\" \"$COLORTERM\" \"$CLICOLOR\" \"${NO_COLOR-unset}\" \"${FORCE_COLOR-unset}\" \"$KEEP_THIS\" \"$*\"\n"
	if err := os.WriteFile(filepath.Join(bin, "bash"), []byte(probe), 0755); err != nil {
		t.Fatal(err)
	}
	command := exec.Command("/bin/sh", "-c", terminalBootstrap)
	command.Env = []string{"PATH=" + bin, "NO_COLOR=1", "FORCE_COLOR=0", "TERM=dumb", "COLORTERM=", "CLICOLOR=0", "KEEP_THIS=retained"}
	output, err := command.CombinedOutput()
	if err != nil {
		t.Fatalf("%s: %v", output, err)
	}
	if string(output) != "xterm-256color\ntruecolor\n1\nunset\nunset\nretained\n-il\n" {
		t.Fatalf("Unexpected interactive environment: %q", output)
	}
}

func TestWorkspaceRoutesRequireAuthentication(t *testing.T) {
	s := &server{token: "secret", microVM: true}
	mux := http.NewServeMux()
	s.registerWorkspaceRoutes(mux)
	for _, route := range []string{"terminal/create", "services/list", "shares/attach", "services/local-ports"} {
		reply := httptest.NewRecorder()
		mux.ServeHTTP(reply, httptest.NewRequest("POST", "/v1/"+route, strings.NewReader(`{"id":"env-test"}`)))
		if reply.Code == 200 || reply.Code == 201 {
			t.Fatalf("unauthenticated %s succeeded", route)
		}
	}
}

func TestTerminalReadOffsetsAndOwnership(t *testing.T) {
	s := &server{}
	s.terminals.Store("term-test", &terminalSession{environment: "env-test", base: 100, output: []byte("hello"), done: true})
	for _, offset := range []int{0, 102, 999} {
		body, _ := json.Marshal(map[string]interface{}{"id": "env-test", "sessionId": "term-test", "offset": offset})
		reply := httptest.NewRecorder()
		s.terminalRead(reply, httptest.NewRequest("POST", "/", bytes.NewReader(body)))
		if reply.Code != 200 {
			t.Fatalf("read failed: %s", reply.Body.String())
		}
		var result struct {
			Data   string
			Offset uint64
			Done   bool
		}
		json.Unmarshal(reply.Body.Bytes(), &result)
		output, _ := base64.StdEncoding.DecodeString(result.Data)
		expected := ""
		if offset == 0 {
			expected = "hello"
		} else if offset == 102 {
			expected = "llo"
		}
		if string(output) != expected || result.Offset != 105 || !result.Done {
			t.Fatalf("invalid bounded read: %+v", result)
		}
	}
	reply := httptest.NewRecorder()
	s.terminalRead(reply, httptest.NewRequest("POST", "/", strings.NewReader(`{"id":"env-other","sessionId":"term-test"}`)))
	if reply.Code != 403 {
		t.Fatal("terminal ownership not enforced")
	}
}

func TestListeningServiceDiscovery(t *testing.T) {
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	port := uint16(listener.Addr().(*net.TCPAddr).Port)
	services, err := listeningServices(os.Getpid())
	if err != nil {
		t.Fatal(err)
	}
	for _, service := range services {
		if service.Port == port {
			if service.Address != "127.0.0.1" {
				t.Fatal("loopback bind address lost")
			}
			return
		}
	}
	t.Fatal("listening TCP service not discovered")
}

func TestHostShareRejectsForeignEndpoints(t *testing.T) {
	s := &server{}
	for _, endpoint := range []string{"http://evil.test:80", "http://10.0.2.2:80@evil.test", "http://10.0.2.2:80/path", "http://10.0.2.2:80?query=1"} {
		body, _ := json.Marshal(hostShareRequest{ID: "env-test", ShareID: "share-test", Endpoint: endpoint, Token: strings.Repeat("a", 64)})
		reply := httptest.NewRecorder()
		s.attachHostShare(reply, httptest.NewRequest("POST", "/", bytes.NewReader(body)))
		if reply.Code != 400 {
			t.Fatalf("endpoint accepted: %s", endpoint)
		}
	}
}
