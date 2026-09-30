package main

import (
	"bytes"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// This client runs inside the microVM. It has only the per-environment
// invitations explicitly installed by the owner, never a host automation socket.
type cliInvitation struct {
	Version     int    `json:"version"`
	Address     string `json:"address"`
	Certificate string `json:"certificate"`
	Token       string `json:"token"`
	ExpiresAt   int64  `json:"expiresAt"`
}
type cliContext struct {
	Name       string        `json:"name"`
	Invitation cliInvitation `json:"invitation"`
}

const cliDirectory = "/var/lib/yougori-cli"

func (s *server) configureCLI(w http.ResponseWriter, r *http.Request) {
	if !s.microVM {
		writeError(w, 409, "The isolated CLI requires a dedicated microVM")
		return
	}
	var request struct {
		ID        string     `json:"id"`
		ContextID string     `json:"contextId"`
		Context   cliContext `json:"context"`
	}
	if !decodeRequest(w, r, &request) || !requireID(w, request.ContextID) {
		return
	}
	if len(request.Context.Name) > 160 || request.Context.Invitation.Version != 1 || request.Context.Invitation.ExpiresAt <= time.Now().Unix() {
		writeError(w, 400, "Invalid or expired environment grant")
		return
	}
	if err := os.MkdirAll(cliDirectory, 0700); err != nil {
		writeError(w, 500, err.Error())
		return
	}
	data, _ := json.Marshal(request.Context)
	file, err := os.CreateTemp(cliDirectory, ".context-")
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	defer os.Remove(file.Name())
	if _, err = file.Write(data); err == nil {
		err = file.Sync()
	}
	closeErr := file.Close()
	if err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Rename(file.Name(), filepath.Join(cliDirectory, request.ContextID+".json"))
	}
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	writeJSON(w, 200, map[string]any{"contextId": request.ContextID, "name": request.Context.Name})
}
func cliCall(invite cliInvitation, method string, params any) (json.RawMessage, error) {
	if invite.ExpiresAt <= time.Now().Unix() {
		return nil, fmt.Errorf("environment access expired; renew it in Yougori Desktop")
	}
	roots := x509.NewCertPool()
	if !roots.AppendCertsFromPEM([]byte(invite.Certificate)) {
		return nil, fmt.Errorf("invalid pinned certificate")
	}
	transport := &http.Transport{TLSClientConfig: &tls.Config{RootCAs: roots, MinVersion: tls.VersionTLS12}, Proxy: nil}
	defer transport.CloseIdleConnections()
	client := &http.Client{Transport: transport, Timeout: 125 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return fmt.Errorf("redirect refused") }}
	data, _ := json.Marshal(map[string]any{"method": method, "params": params})
	request, err := http.NewRequest(http.MethodPost, "https://"+invite.Address+"/rpc", bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	request.Header.Set("Authorization", "Bearer "+invite.Token)
	response, err := client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, 8*1024*1024+1))
	if err != nil {
		return nil, err
	}
	if len(body) > 8*1024*1024 {
		return nil, fmt.Errorf("response exceeded 8 MiB")
	}
	if response.StatusCode != 200 {
		var failure struct {
			Error string `json:"error"`
		}
		_ = json.Unmarshal(body, &failure)
		return nil, fmt.Errorf("access refused: %s", failure.Error)
	}
	return body, nil
}
func cliQuote(value string) string { return "'" + strings.ReplaceAll(value, "'", "'\"'\"'") + "'" }
func isolatedCLI(args []string) int {
	fail := func(err error) int { fmt.Fprintln(os.Stderr, err); return 1 }
	if len(args) == 0 || args[0] == "help" || args[0] == "--help" {
		fmt.Println("Yougori isolated CLI\n  ps | context list\n  inspect|logs|start|stop|restart ENVIRONMENT_ID\n  exec ENVIRONMENT_ID -- PROGRAM [ARGUMENTS...]\n\nOnly explicitly granted environments are available. This shell and its files live inside the CLI microVM. Manage My PC permissions and renew/revoke environment grants in Yougori Desktop.")
		return 0
	}
	if args[0] == "version" || args[0] == "--version" {
		fmt.Println("yougori isolated CLI 1.0 (sharing protocol 1)")
		return 0
	}
	files, err := filepath.Glob(filepath.Join(cliDirectory, "env-*.json"))
	if err != nil {
		return fail(err)
	}
	contexts := map[string]cliContext{}
	for _, file := range files {
		data, err := os.ReadFile(file)
		if err != nil {
			return fail(err)
		}
		var context cliContext
		if err = json.Unmarshal(data, &context); err != nil {
			return fail(err)
		}
		contexts[strings.TrimSuffix(filepath.Base(file), ".json")] = context
	}
	if args[0] == "ps" || (len(args) == 2 && args[0] == "context" && args[1] == "list") {
		ids := []string{}
		for id := range contexts {
			ids = append(ids, id)
		}
		sort.Strings(ids)
		for _, id := range ids {
			c := contexts[id]
			result, err := cliCall(c.Invitation, "inspect", nil)
			if err != nil {
				fmt.Printf("%s\t%s\tUnavailable: %v\n", id, c.Name, err)
			} else {
				fmt.Printf("%s\t%s\t%s\n", id, c.Name, result)
			}
		}
		return 0
	}
	if len(args) < 2 {
		return fail(fmt.Errorf("environment ID is required; run yougori ps"))
	}
	context, ok := contexts[args[1]]
	if !ok {
		return fail(fmt.Errorf("this environment has not been granted to the isolated CLI"))
	}
	method := args[0]
	params := map[string]any{}
	switch method {
	case "exec":
		if len(args) < 4 || args[2] != "--" {
			return fail(fmt.Errorf("usage: yougori exec ENV_ID -- PROGRAM [ARGUMENTS...]"))
		}
		quoted := []string{}
		for _, arg := range args[3:] {
			quoted = append(quoted, cliQuote(arg))
		}
		params["command"] = strings.Join(quoted, " ")
	case "logs":
		method = "console"
	case "start", "stop", "restart":
		if len(args) != 2 {
			return fail(fmt.Errorf("unexpected arguments"))
		}
		if method == "restart" {
			if _, err = cliCall(context.Invitation, "power", map[string]any{"status": "stopped"}); err != nil {
				return fail(err)
			}
		}
		params["status"] = "running"
		if method == "stop" {
			params["status"] = "stopped"
		}
		method = "power"
	case "inspect":
		if len(args) != 2 {
			return fail(fmt.Errorf("unexpected arguments"))
		}
	default:
		return fail(fmt.Errorf("unsupported isolated operation; use yougori help"))
	}
	result, err := cliCall(context.Invitation, method, params)
	if err != nil {
		return fail(err)
	}
	if method == "exec" {
		var output struct {
			Stdout   string `json:"stdout"`
			Stderr   string `json:"stderr"`
			ExitCode int    `json:"exitCode"`
		}
		if err = json.Unmarshal(result, &output); err != nil {
			return fail(err)
		}
		fmt.Print(output.Stdout)
		fmt.Fprint(os.Stderr, output.Stderr)
		if output.ExitCode < 0 || output.ExitCode > 255 {
			return 1
		}
		return output.ExitCode
	}
	if method == "console" {
		var text string
		if err = json.Unmarshal(result, &text); err != nil {
			return fail(err)
		}
		fmt.Print(text)
	} else {
		fmt.Println(string(result))
	}
	return 0
}
