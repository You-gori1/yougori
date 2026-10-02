package main

import (
	"archive/tar"
	"bytes"
	"context"
	"io"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestImportInactivityClosesStreamAndRecordsFailure(t *testing.T) {
	s := &server{}
	reader, writer := io.Pipe()
	defer writer.Close()
	ctx, state, finish, err := s.beginImport(context.Background(), "env-stall", "a", reader, 40*time.Millisecond)
	if err != nil {
		t.Fatal(err)
	}
	defer finish()
	blocked := make(chan error, 1)
	go func() { var b [1]byte; _, err := reader.Read(b[:]); blocked <- err }()
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("inactivity did not cancel")
	}
	select {
	case err := <-blocked:
		if err == nil {
			t.Fatal("body still open")
		}
	case <-time.After(time.Second):
		t.Fatal("reader remained blocked")
	}
	if state.snapshot()["errorCode"] != "transfer_inactive" {
		t.Fatal(state.snapshot())
	}
}
func TestImportCancellationIsScopedAndDoesNotBlockLifecycle(t *testing.T) {
	s := &server{}
	a, aw := io.Pipe()
	b, bw := io.Pipe()
	defer aw.Close()
	defer bw.Close()
	first, _, finishA, err := s.beginImport(context.Background(), "env-a", "a", a, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer finishA()
	second, _, finishB, err := s.beginImport(context.Background(), "env-b", "b", b, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	defer finishB()
	request := httptest.NewRequest("POST", "/v1/files/import/cancel", strings.NewReader(`{"id":"env-a","transfer":"a"}`))
	response := httptest.NewRecorder()
	s.cancelImport(response, request)
	if response.Code != 200 {
		t.Fatal(response.Body.String())
	}
	select {
	case <-first.Done():
	case <-time.After(time.Second):
		t.Fatal("cancel did not settle")
	}
	if second.Err() != nil {
		t.Fatal("cancelled an unrelated transfer")
	}
	// This is the actual container lock used by start/stop; the transfer state
	// and control endpoints never take it, even while a body is stalled.
	unlocked := make(chan struct{})
	go func() { release := s.locks.lock(containerLockKey("env-a")); release(); close(unlocked) }()
	select {
	case <-unlocked:
	case <-time.After(time.Second):
		t.Fatal("transfer blocked lifecycle")
	}
}
func TestImportEndpointReleasesContainerLockDuringRealExtraction(t *testing.T) {
	if os.Geteuid() != 0 {
		t.Skip("guest agent runs as root; run this real endpoint fixture as root")
	}
	s := &server{microVM: true}
	folder := t.TempDir()
	original := filepath.Join(folder, "keep")
	os.WriteFile(original, []byte("original"), 0600)
	archive := importArchive(t, &tar.Header{Name: "new-file", Typeflag: tar.TypeReg, Size: 4096, Mode: 0600})
	reader, writer := io.Pipe()
	request := httptest.NewRequest("POST", "/v1/files/import?id=env-fixture&transfer="+strings.Repeat("a", 32)+"&folder="+folder, reader)
	request.ContentLength = int64(len(archive))
	response := httptest.NewRecorder()
	finished := make(chan struct{})
	go func() { s.importFiles(response, request); close(finished) }()
	prefixSent := make(chan struct{})
	go func() { writer.Write(archive[:1024]); close(prefixSent) }()
	select {
	case <-prefixSent:
	case <-finished:
		t.Fatalf("copy failed before streaming: %s", response.Body.String())
	case <-time.After(3 * time.Second):
		t.Fatal("extraction did not start")
	}
	unlocked := make(chan struct{})
	go func() { release := s.locks.lock(containerLockKey("env-fixture")); release(); close(unlocked) }()
	select {
	case <-unlocked:
	case <-time.After(time.Second):
		t.Fatal("extraction retained container lock")
	}
	cancelRequest := httptest.NewRequest("POST", "/v1/files/import/cancel", strings.NewReader(`{"id":"env-fixture","transfer":"`+strings.Repeat("a", 32)+`"}`))
	s.cancelImport(httptest.NewRecorder(), cancelRequest)
	select {
	case <-finished:
	case <-time.After(3 * time.Second):
		t.Fatal("cancel did not interrupt real tar extraction")
	}
	writer.Close()
	if data, _ := os.ReadFile(original); !bytes.Equal(data, []byte("original")) {
		t.Fatal("modified an existing guest file")
	}
	if _, err := os.Stat(filepath.Join(folder, "yougori-import-"+strings.Repeat("a", 32), "new-file")); err != nil {
		t.Fatal("partial copy was not retained", err)
	}
}
