package main

// A snapshot keeps the container paused for consistency, but its transfer must
// never prevent lifecycle control. Cancellation interrupts both the exporter
// and a blocked HTTP writer before Stop waits for the container lock.
import (
	"context"
	"fmt"
	"io"
	"net/http"
	"sync"
	"time"
)

const snapshotExportIdleTimeout = 90 * time.Second
const snapshotPrepareTimeout = 5 * time.Minute
const snapshotExportTotalTimeout = 12 * time.Hour

type snapshotExportState struct {
	mu       sync.Mutex
	id       string
	snapshot string
	changed  time.Time
	sending  bool
	cancel   context.CancelFunc
}

type snapshotExportManager struct {
	mu       sync.Mutex
	active   map[string]*snapshotExportState
	controls map[string]int
}

func (s *server) beginSnapshotExport(parent context.Context, w http.ResponseWriter, id, snapshot string) (context.Context, io.Writer, func(), error) {
	return s.beginSnapshotExportWithTimeout(parent, w, id, snapshot, snapshotExportIdleTimeout)
}

func (s *server) beginSnapshotExportWithTimeout(parent context.Context, w http.ResponseWriter, id, snapshot string, idle time.Duration) (context.Context, io.Writer, func(), error) {
	manager := &s.snapshotExports
	manager.mu.Lock()
	if manager.controls[id] > 0 || manager.active[id] != nil {
		manager.mu.Unlock()
		return nil, nil, nil, fmt.Errorf("lifecycle control or another snapshot export is active for this environment")
	}
	if len(manager.active) >= 256 {
		manager.mu.Unlock()
		return nil, nil, nil, fmt.Errorf("snapshot export capacity reached")
	}
	if manager.active == nil {
		manager.active = make(map[string]*snapshotExportState)
	}
	ctx, cancel := context.WithTimeout(parent, snapshotExportTotalTimeout)
	state := &snapshotExportState{id: id, snapshot: snapshot, changed: time.Now(), cancel: cancel}
	manager.active[id] = state
	manager.mu.Unlock()
	controller := http.NewResponseController(w)
	done, watched := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(watched)
		ticker := time.NewTicker(min(idle/4, time.Second))
		defer ticker.Stop()
		for {
			select {
			case <-done:
				return
			case <-ctx.Done():
				// Context cancellation alone does not unblock a connected client
				// which stopped reading. The standard HTTP server implements this.
				state.mu.Lock()
				_ = controller.SetWriteDeadline(time.Now())
				state.mu.Unlock()
				return
			case <-ticker.C:
				state.mu.Lock()
				limit := snapshotPrepareTimeout
				if state.sending {
					limit = idle
				}
				stalled := time.Since(state.changed) >= limit
				state.mu.Unlock()
				if stalled {
					cancel()
				}
			}
		}
	}()
	var finishOnce sync.Once
	finish := func() {
		finishOnce.Do(func() {
			close(done)
			<-watched
			cancel()
			_ = controller.SetWriteDeadline(time.Time{})
			manager.mu.Lock()
			if manager.active[id] == state {
				delete(manager.active, id)
			}
			manager.mu.Unlock()
		})
	}
	return ctx, snapshotExportWriter{ctx: ctx, writer: w, controller: controller, state: state, idle: idle}, finish, nil
}

type snapshotExportWriter struct {
	ctx        context.Context
	writer     io.Writer
	controller *http.ResponseController
	state      *snapshotExportState
	idle       time.Duration
}

func (writer snapshotExportWriter) Write(data []byte) (int, error) {
	writer.state.mu.Lock()
	if err := writer.ctx.Err(); err != nil {
		writer.state.mu.Unlock()
		return 0, err
	}
	writer.state.sending = true
	_ = writer.controller.SetWriteDeadline(time.Now().Add(writer.idle))
	writer.state.mu.Unlock()
	n, err := writer.writer.Write(data)
	if n > 0 {
		writer.state.mu.Lock()
		writer.state.changed = time.Now()
		writer.state.mu.Unlock()
	}
	return n, err
}

// The barrier prevents a new export from slipping between cancellation and
// lock acquisition. It remains until the complete lifecycle action returns.
func (s *server) snapshotLifecycleControl(id string) func() {
	manager := &s.snapshotExports
	manager.mu.Lock()
	if manager.controls == nil {
		manager.controls = make(map[string]int)
	}
	manager.controls[id]++
	if state := manager.active[id]; state != nil {
		state.cancel()
	}
	manager.mu.Unlock()
	return func() {
		manager.mu.Lock()
		manager.controls[id]--
		if manager.controls[id] == 0 {
			delete(manager.controls, id)
		}
		manager.mu.Unlock()
	}
}

func (s *server) cancelSnapshotExport(w http.ResponseWriter, r *http.Request) {
	var request snapshotRequest
	if !decodeRequest(w, r, &request) || !requireID(w, request.ID) || !requireID(w, request.SnapshotID) {
		return
	}
	manager := &s.snapshotExports
	manager.mu.Lock()
	state := manager.active[request.ID]
	active := state != nil && state.snapshot == request.SnapshotID
	if active {
		state.cancel()
	}
	manager.mu.Unlock()
	writeJSON(w, http.StatusAccepted, map[string]any{"cancelRequested": active, "environmentId": request.ID, "snapshotId": request.SnapshotID, "partialCopyPolicy": "discard_unverified_snapshot"})
}
