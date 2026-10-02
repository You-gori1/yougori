package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"
)

// Control calls never take the container lifecycle lock. Staged files remain
// in a unique import directory on failure, and never replace application files.
type importKey struct {
	server   *server
	transfer string
}
type importState struct {
	mu                                     sync.Mutex
	id, transfer, phase, status, errorCode string
	received, confirmed                    int64
	changed                                time.Time
	cancel                                 context.CancelFunc
}

var imports = struct {
	sync.Mutex
	states map[importKey]*importState
}{states: make(map[importKey]*importState)}

const importIdleTimeout = 90 * time.Second
const importTotalTimeout = 12 * time.Hour

func (state *importState) progress(received, confirmed int64) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if received > 0 {
		state.received += received
	}
	if confirmed > 0 {
		state.confirmed += confirmed
	}
	if received > 0 || confirmed > 0 {
		state.changed = time.Now()
	}
}
func (state *importState) snapshot() map[string]interface{} {
	state.mu.Lock()
	defer state.mu.Unlock()
	return map[string]interface{}{"transferId": state.transfer, "environmentId": state.id, "phase": state.phase, "status": state.status, "receivedBytes": state.received, "confirmedBytes": state.confirmed, "lastProgressAt": state.changed.UTC().Format(time.RFC3339Nano), "errorCode": state.errorCode, "partialCopyPolicy": "preserve_unique_guest_directory"}
}
func (s *server) beginImport(parent context.Context, id, transfer string, body io.ReadCloser, idle time.Duration) (context.Context, *importState, func(), error) {
	imports.Lock()
	for key, state := range imports.states {
		state.mu.Lock()
		active := state.status == "running"
		expired := !active && time.Since(state.changed) > 30*time.Minute
		same := key.server == s && state.id == id
		state.mu.Unlock()
		if expired {
			delete(imports.states, key)
		}
		if active && same {
			imports.Unlock()
			return nil, nil, nil, fmt.Errorf("another transfer is active for this environment; inspect or cancel it")
		}
	}
	if len(imports.states) >= 256 {
		for key, state := range imports.states {
			state.mu.Lock()
			done := state.status != "running"
			state.mu.Unlock()
			if done {
				delete(imports.states, key)
				break
			}
		}
	}
	if len(imports.states) >= 256 {
		imports.Unlock()
		return nil, nil, nil, fmt.Errorf("transfer registry is full; cancel active work")
	}
	key := importKey{s, transfer}
	if imports.states[key] != nil {
		imports.Unlock()
		return nil, nil, nil, fmt.Errorf("transfer identifier already used; inspect its outcome before retrying")
	}
	ctx, cancel := context.WithTimeout(parent, importTotalTimeout)
	state := &importState{id: id, transfer: transfer, phase: "connecting", status: "running", changed: time.Now(), cancel: cancel}
	imports.states[key] = state
	imports.Unlock()
	done := make(chan struct{})
	go func() {
		ticker := time.NewTicker(idle / 4)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				body.Close()
				return
			case <-done:
				return
			case <-ticker.C:
				state.mu.Lock()
				stalled := time.Since(state.changed) >= idle
				if stalled {
					state.errorCode = "transfer_inactive"
				}
				state.mu.Unlock()
				if stalled {
					cancel()
				}
			}
		}
	}()
	finish := func() {
		close(done)
		cancel()
		state.mu.Lock()
		if state.status == "running" {
			state.status = "failed"
			if state.errorCode == "" {
				state.errorCode = "transfer_incomplete"
			}
		}
		state.mu.Unlock()
	}
	return ctx, state, finish, nil
}

type importReader struct {
	context context.Context
	reader  io.Reader
	state   *importState
}

func (r importReader) Read(p []byte) (int, error) {
	if err := r.context.Err(); err != nil {
		return 0, err
	}
	n, err := r.reader.Read(p)
	r.state.progress(int64(n), 0)
	return n, err
}

type importWriter struct {
	context  context.Context
	writer   io.Writer
	progress func(int64)
}

func (w importWriter) Write(p []byte) (int, error) {
	if err := w.context.Err(); err != nil {
		return 0, err
	}
	n, err := w.writer.Write(p)
	if n > 0 && w.progress != nil {
		w.progress(int64(n))
	}
	return n, err
}

func (s *server) importProgress(w http.ResponseWriter, r *http.Request) {
	id, transfer := r.URL.Query().Get("id"), r.URL.Query().Get("transfer")
	if !requireID(w, id) {
		return
	}
	imports.Lock()
	state := imports.states[importKey{s, transfer}]
	imports.Unlock()
	if state == nil || state.id != id {
		writeError(w, 404, "transfer not found for this environment")
		return
	}
	writeJSON(w, 200, state.snapshot())
}
func (s *server) cancelImport(w http.ResponseWriter, r *http.Request) {
	var request struct {
		ID       string `json:"id"`
		Transfer string `json:"transfer"`
	}
	if json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&request) != nil || !requireID(w, request.ID) {
		writeError(w, 400, "invalid transfer cancellation")
		return
	}
	imports.Lock()
	state := imports.states[importKey{s, request.Transfer}]
	imports.Unlock()
	if state == nil || state.id != request.ID {
		writeError(w, 404, "transfer not found for this environment")
		return
	}
	state.mu.Lock()
	active := state.status == "running"
	if active {
		state.errorCode = "operation_cancelled"
		state.status = "cancelled"
	}
	state.mu.Unlock()
	if active {
		state.cancel()
	}
	result := state.snapshot()
	result["cancelRequested"] = active
	writeJSON(w, 200, result)
}
