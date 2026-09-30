package main

import (
	"encoding/json"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestProvisionLogsBeforeContainerExists(t *testing.T) {
	s := &server{}
	progress := &provisionLog{}
	progress.Write([]byte("Downloading image layers"))
	s.provisionLogs.Store("env-pending", progress)
	request := httptest.NewRequest("POST", "/v1/containers/logs", strings.NewReader(`{"id":"env-pending","tail":200}`))
	response := httptest.NewRecorder()
	s.workloadLogs(response, request)
	if response.Code != 200 {
		t.Fatalf("status %d: %s", response.Code, response.Body.String())
	}
	var result struct {
		Logs string `json:"logs"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	if result.Logs != "Downloading image layers" {
		t.Fatal(result.Logs)
	}
}

func TestProvisionLogIsBounded(t *testing.T) {
	progress := &provisionLog{}
	progress.Write([]byte(strings.Repeat("x", 10000)))
	progress.Write([]byte("latest"))
	if len(progress.text()) != 8192 || !strings.HasSuffix(progress.text(), "latest") {
		t.Fatal("incorrect tail")
	}
}
