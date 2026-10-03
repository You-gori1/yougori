package main

import (
	"bytes"
	"compress/gzip"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// A connected client which stops reading. ResponseController must interrupt
// the blocked write; merely cancelling the request context is insufficient.
type stalledSnapshotWriter struct {
	header  http.Header
	bytes   bytes.Buffer
	writes  int
	blocked chan struct{}
	release chan struct{}
	once    sync.Once
}

func newStalledSnapshotWriter() *stalledSnapshotWriter {
	return &stalledSnapshotWriter{header: make(http.Header), blocked: make(chan struct{}), release: make(chan struct{})}
}
func (writer *stalledSnapshotWriter) Header() http.Header { return writer.header }
func (writer *stalledSnapshotWriter) WriteHeader(int)     {}
func (writer *stalledSnapshotWriter) Write(data []byte) (int, error) {
	writer.writes++
	// Preserve the stream header and metadata; block the gzip payload while
	// the actual exporter process and consistency pause are still active.
	if writer.writes >= 4 {
		writer.once.Do(func() { close(writer.blocked) })
		<-writer.release
		return 0, errors.New("client write interrupted")
	}
	return writer.bytes.Write(data)
}
func (writer *stalledSnapshotWriter) SetWriteDeadline(deadline time.Time) error {
	if !deadline.IsZero() && !deadline.After(time.Now()) {
		select {
		case <-writer.release:
		default:
			close(writer.release)
		}
	}
	return nil
}

func TestSnapshotStopInterruptsBlockedExportAndResumesBeforeLifecycle(t *testing.T) {
	if os.Geteuid() != 0 {
		t.Skip("guest exports prepare private staging under /run; run this endpoint fixture as root")
	}
	root := t.TempDir()
	logfile := filepath.Join(root, "commands")
	adapter := `#!/bin/sh
shift 2
case "$1" in
inspect) printf '%s' '{"Id":"env-snapshot-fixture","Config":{},"Image":"fixture","State":{"Running":true,"Paused":false}}';;
image) printf '%s' '{"architecture":"amd64","os":"linux","config":{}}';;
pause|unpause|stop) printf '%s\n' "$1" >> "$YOUGORI_AUDIT_LOG";;
export) exec head -c 1048576 /dev/urandom;;
*) exit 1;;
esac
`
	if err := os.WriteFile(filepath.Join(root, "nerdctl"), []byte(adapter), 0700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", root+":"+os.Getenv("PATH"))
	t.Setenv("YOUGORI_AUDIT_LOG", logfile)
	s := &server{}
	writer := newStalledSnapshotWriter()
	exported := make(chan struct{})
	go func() {
		defer close(exported)
		s.exportSnapshotWithConfiguration(writer, httptest.NewRequest("POST", "/v1/snapshots/export", strings.NewReader(`{"id":"env-snapshot-fixture","snapshotId":"snapshot-fixture"}`)), func(context.Context, string, json.RawMessage, json.RawMessage) (json.RawMessage, error) {
			return json.RawMessage(`{"Cmd":["sleep","2147483647"]}`), nil
		})
	}()
	select {
	case <-writer.blocked:
	case <-exported:
		t.Fatalf("export ended before reaching the stalled client: %s", writer.bytes.String())
	case <-time.After(5 * time.Second):
		t.Fatal("export did not start")
	}
	stopped := make(chan *httptest.ResponseRecorder, 1)
	begin := time.Now()
	go func() {
		reply := httptest.NewRecorder()
		s.action(reply, httptest.NewRequest("POST", "/v1/containers/action", strings.NewReader(`{"id":"env-snapshot-fixture","action":"stop"}`)))
		stopped <- reply
	}()
	select {
	case reply := <-stopped:
		if reply.Code != 200 || time.Since(begin) > 3*time.Second {
			t.Fatalf("stop was not bounded: %d %s", reply.Code, reply.Body.String())
		}
	case <-time.After(4 * time.Second):
		s.snapshotLifecycleControl("env-snapshot-fixture")()
		t.Fatal("Stop waited behind an ordinary stalled snapshot transfer")
	}
	select {
	case <-exported:
	case <-time.After(time.Second):
		t.Fatal("snapshot lock or exporter remained active")
	}
	commands, err := os.ReadFile(logfile)
	if err != nil || string(commands) != "pause\nunpause\nstop\n" {
		t.Fatalf("snapshot consistency pause was not restored before Stop: %q, %v", commands, err)
	}
	data := bytes.NewReader(writer.bytes.Bytes())
	if string(makeRead(t, data, len(snapshotStreamMagic))) != snapshotStreamMagic {
		t.Fatal("snapshot did not reach its streaming phase")
	}
	var size uint32
	if err := binary.Read(data, binary.BigEndian, &size); err != nil {
		t.Fatal(err)
	}
	makeRead(t, data, int(size))
	compressed, err := gzip.NewReader(data)
	if err == nil {
		_, err = io.ReadAll(compressed)
	}
	if err == nil {
		t.Fatal("cancelled export acquired a valid gzip completion footer")
	}
}

func makeRead(t *testing.T, reader io.Reader, size int) []byte {
	t.Helper()
	data := make([]byte, size)
	if _, err := io.ReadFull(reader, data); err != nil {
		t.Fatal(err)
	}
	return data
}

func TestSnapshotCancellationIsScopedAndPreventsLifecycleRace(t *testing.T) {
	s, other := &server{}, &server{}
	ctx, _, finish, err := s.beginSnapshotExport(context.Background(), httptest.NewRecorder(), "env-one", "snapshot-one")
	if err != nil {
		t.Fatal(err)
	}
	defer finish()
	unrelated, _, finishOther, err := other.beginSnapshotExport(context.Background(), httptest.NewRecorder(), "env-one", "snapshot-one")
	if err != nil {
		t.Fatal(err)
	}
	defer finishOther()
	s.cancelSnapshotExport(httptest.NewRecorder(), httptest.NewRequest("POST", "/v1/snapshots/export/cancel", strings.NewReader(`{"id":"env-one","snapshotId":"wrong"}`)))
	if ctx.Err() != nil {
		t.Fatal("an unrelated snapshot identifier cancelled this export")
	}
	release := s.snapshotLifecycleControl("env-one")
	defer release()
	if ctx.Err() == nil || unrelated.Err() != nil {
		t.Fatal("lifecycle cancellation was not scoped to its exact server and environment")
	}
	finish()
	if _, _, _, err := s.beginSnapshotExport(context.Background(), httptest.NewRecorder(), "env-one", "snapshot-new"); err == nil {
		t.Fatal("new export slipped between cancellation and the lifecycle lock")
	}
}

func TestSnapshotInactivityInterruptsConnectedWriter(t *testing.T) {
	s := &server{}
	writer := newStalledSnapshotWriter()
	ctx, output, finish, err := s.beginSnapshotExportWithTimeout(context.Background(), writer, "env-idle", "snapshot-idle", 40*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	defer finish()
	for i := 0; i < 3; i++ {
		if _, err := output.Write([]byte("header")); err != nil {
			t.Fatal(err)
		}
	}
	result := make(chan error, 1)
	go func() { _, err := output.Write([]byte("blocked")); result <- err }()
	select {
	case err := <-result:
		if err == nil || ctx.Err() == nil {
			t.Fatal("inactivity completed without a cancelled stream")
		}
	case <-time.After(time.Second):
		t.Fatal("connected but inactive snapshot writer stayed blocked")
	}
}
