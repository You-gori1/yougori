package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os/exec"
	"strings"
	"time"
)

type containerState struct {
	Status    string `json:"Status"`
	Running   bool   `json:"Running"`
	Paused    bool   `json:"Paused"`
	StartedAt string `json:"StartedAt"`
	ExitCode  int    `json:"ExitCode"`
	OOMKilled bool   `json:"OOMKilled"`
	Error     string `json:"Error"`
}

func (state containerState) live() bool {
	switch strings.ToLower(strings.TrimSpace(state.Status)) {
	case "running", "paused", "pausing", "restarting":
		return true
	}
	return state.Running || state.Paused
}

// Keep only the tail even if one log line is huge.
type logTail struct{ data []byte }

func (b *logTail) Write(p []byte) (int, error) {
	n := len(p)
	const limit = 8192
	if len(p) >= limit {
		b.data = append(b.data[:0], p[len(p)-limit:]...)
		return n, nil
	}
	if extra := len(b.data) + len(p) - limit; extra > 0 {
		copy(b.data, b.data[extra:])
		b.data = b.data[:len(b.data)-extra]
	}
	b.data = append(b.data, p...)
	return n, nil
}

func failureDescription(state containerState, logs string) string {
	if state.live() {
		return ""
	}
	message := fmt.Sprintf("Container exited with code %d.", state.ExitCode)
	if state.OOMKilled {
		message += " It ran out of its assigned memory. Increase the memory allocation before retrying."
	}
	if state.ExitCode == 0 {
		message += " Its startup command finished. Use a long-running service or a keep-alive command for a terminal workspace."
	}
	if state.ExitCode == 132 {
		message += " The program used a CPU instruction unavailable in the runtime."
	}
	if state.Error != "" {
		message += " " + state.Error
	}
	if logs = strings.TrimSpace(logs); logs != "" {
		message += "\nRecent container output:\n" + logs
	}
	return message
}

func (s *server) containerDiagnostics(w http.ResponseWriter, r *http.Request) {
	id := strings.TrimPrefix(r.URL.Path, "/v1/containers/status/")
	if !requireID(w, id) {
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
	defer cancel()
	output, err := run(ctx, "nerdctl", "--namespace", namespace, "inspect", "--format", "{{json .State}}", id)
	if err != nil {
		writeCommandError(w, err)
		return
	}
	var state containerState
	if err := json.Unmarshal([]byte(output.Stdout), &state); err != nil {
		writeError(w, 500, "Invalid container status: "+err.Error())
		return
	}
	var tail logTail
	if !state.live() {
		args := []string{"--namespace", namespace, "logs", "--tail", "40"}
		if started, err := time.Parse(time.RFC3339Nano, state.StartedAt); err == nil && !started.IsZero() {
			args = append(args, "--since", state.StartedAt)
		}
		command := exec.CommandContext(ctx, "nerdctl", append(args, id)...)
		command.Stdout = &tail
		command.Stderr = &tail
		_ = command.Run()
	}
	writeJSON(w, 200, map[string]any{"running": state.live(), "paused": state.Paused || state.Status == "pausing", "exitCode": state.ExitCode, "cleanExit": state.Status == "exited" && state.ExitCode == 0 && !state.OOMKilled && state.Error == "", "message": failureDescription(state, string(tail.data))})
}
