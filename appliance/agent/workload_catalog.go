package main

import (
	"context"
	"encoding/json"
	"net/http"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"time"
)

// A bounded, synchronized output tail lets log readers observe creation before
// the container exists. Command arguments and environment variables are not logged.
type provisionLog struct {
	mu   sync.Mutex
	tail logTail
}

func (p *provisionLog) Write(data []byte) (int, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.tail.Write(data)
}
func (p *provisionLog) text() string {
	p.mu.Lock()
	defer p.mu.Unlock()
	return string(p.tail.data)
}

func (s *server) workloadLogs(w http.ResponseWriter, r *http.Request) {
	var request struct {
		ID   string `json:"id"`
		Tail int    `json:"tail"`
	}
	if !decodeRequest(w, r, &request) || !requireID(w, request.ID) {
		return
	}
	if request.Tail < 1 || request.Tail > 1000 {
		writeError(w, 400, "tail must be between 1 and 1000")
		return
	}
	if progress, ok := s.provisionLogs.Load(request.ID); ok {
		writeJSON(w, 200, map[string]any{"logs": progress.(*provisionLog).text(), "limitBytes": 8192})
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), s.diagnosticTimeout())
	defer cancel()
	var tail logTail
	command := exec.CommandContext(ctx, "nerdctl", "--namespace", namespace, "logs", "--tail", strconv.Itoa(request.Tail), request.ID)
	command.Stdout = &tail
	command.Stderr = &tail
	if err := command.Run(); err != nil {
		writeError(w, 500, "Cannot read workload logs: "+string(tail.data))
		return
	}
	writeJSON(w, 200, map[string]any{"logs": string(tail.data), "limitBytes": 8192})
}

func (s *server) workloadImages(w http.ResponseWriter, r *http.Request) {
	var request struct {
		Action string `json:"action"`
		Image  string `json:"image"`
	}
	if !decodeRequest(w, r, &request) {
		return
	}
	if s.microVM {
		writeError(w, 409, "OCI image management is available in the container engine")
		return
	}
	if request.Action != "list" && (request.Image == "" || len(request.Image) > 512 || strings.HasPrefix(request.Image, "-") || strings.ContainsAny(request.Image, " \t\r\n\x00")) {
		writeError(w, 400, "Invalid OCI image reference")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 15*time.Minute)
	defer cancel()
	args := []string{"--namespace", namespace}
	switch request.Action {
	case "list":
		args = append(args, "images", "--format", "{{json .}}")
	case "pull":
		args = append(args, "pull", request.Image)
	case "remove":
		args = append(args, "image", "rm", request.Image)
	default:
		writeError(w, 400, "Unsupported image action")
		return
	}
	unlock := s.locks.lock(imageLockKey(request.Image))
	defer unlock()
	output, err := run(ctx, "nerdctl", args...)
	if err != nil {
		writeCommandError(w, err)
		return
	}
	if request.Action == "remove" {
		s.knownImages.Delete(request.Image)
	}
	if request.Action == "list" {
		entries := []any{}
		for _, line := range strings.Split(strings.TrimSpace(output.Stdout), "\n") {
			if strings.TrimSpace(line) == "" {
				continue
			}
			var entry any
			if err := json.Unmarshal([]byte(line), &entry); err != nil {
				writeError(w, 500, "Cannot decode image catalog")
				return
			}
			entries = append(entries, entry)
		}
		writeJSON(w, 200, entries)
		return
	}
	writeJSON(w, 200, output)
}
