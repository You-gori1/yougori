package main

import (
	"encoding/base64"
	"fmt"
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
	"sync"
	"testing"
)

func TestRemoteFilesScopesAndPermissions(t *testing.T) {
	folder := t.TempDir()
	outside := t.TempDir()
	if err := os.WriteFile(filepath.Join(folder, "hello"), []byte("hello"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(folder, "nested"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(outside, "secret"), []byte("private"), 0600); err != nil {
		t.Fatal(err)
	}
	root, err := unix.Open(folder, unix.O_RDONLY|unix.O_DIRECTORY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(root)
	listed, err := remoteFileOperation(root, remoteFileRequest{Operation: "list", ReadOnly: true})
	if err != nil {
		t.Fatal(err)
	}
	entries := listed.(map[string]interface{})["entries"].([]map[string]interface{})
	if len(entries) != 2 {
		t.Fatalf("guest entries missing from directory listing: %#v", entries)
	}
	found := map[string]bool{}
	for _, entry := range entries {
		found[entry["name"].(string)] = entry["directory"].(bool)
	}
	if _, ok := found["hello"]; !ok || !found["nested"] {
		t.Fatalf("wrong guest entries in directory listing: %#v", entries)
	}
	r := remoteFileRequest{Operation: "read", Path: "hello", ReadOnly: true, Length: 5}
	value, err := remoteFileOperation(root, r)
	if err != nil {
		t.Fatal(err)
	}
	if value.(map[string]string)["data"] != "aGVsbG8=" {
		t.Fatal(value)
	}
	r.Operation = "write"
	r.Data = "eA=="
	if _, err = remoteFileOperation(root, r); err == nil {
		t.Fatal("read-only write allowed")
	}
	for _, name := range []string{"../secret", "/etc/passwd", "nested/../../secret", "a\\b"} {
		r.Path = name
		if _, err = remoteFileOperation(root, r); err == nil {
			t.Fatal(name)
		}
	}
	if err = os.Symlink(outside, filepath.Join(folder, "escape")); err != nil {
		t.Fatal(err)
	}
	r = remoteFileRequest{Operation: "read", Path: "escape/secret", Length: 10}
	if _, err = remoteFileOperation(root, r); err == nil {
		t.Fatal("symlink escape allowed")
	}
	if err = os.Link(filepath.Join(outside, "secret"), filepath.Join(folder, "hard")); err != nil {
		t.Fatal(err)
	}
	r.Path = "hard"
	if _, err = remoteFileOperation(root, r); err == nil {
		t.Fatal("hard link allowed")
	}
	r = remoteFileRequest{Operation: "create", Path: "new"}
	if _, err = remoteFileOperation(root, r); err != nil {
		t.Fatal(err)
	}
	r.Operation = "write"
	r.Data = "aGVsbG8="
	if _, err = remoteFileOperation(root, r); err != nil {
		t.Fatal(err)
	}
	data, _ := os.ReadFile(filepath.Join(folder, "new"))
	if string(data) != "hello" {
		t.Fatal(string(data))
	}
	r.Operation = "replace"
	r.ExpectedData = "aGVsbG8="
	r.Data = "bmV3"
	if _, err = remoteFileOperation(root, r); err != nil {
		t.Fatal(err)
	}
	if _, err = remoteFileOperation(root, r); err == nil {
		t.Fatal("stale edit overwrote file")
	}
	r.Operation = "remove"
	r.Path = ""
	if _, err = remoteFileOperation(root, r); err == nil {
		t.Fatal("root deletion allowed")
	}
}

func TestRemoteConcurrentTextSavesRejectStaleEdits(t *testing.T) {
	folder := t.TempDir()
	original := []byte("the version both editors opened")
	if err := os.Mkdir(filepath.Join(folder, "nested"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(folder, "nested", "edited"), original, 0600); err != nil {
		t.Fatal(err)
	}
	root, err := unix.Open(folder, unix.O_RDONLY|unix.O_DIRECTORY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(root)
	nested, err := unix.Open(filepath.Join(folder, "nested"), unix.O_RDONLY|unix.O_DIRECTORY, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unix.Close(nested)
	start := make(chan struct{})
	results := make(chan error, 32)
	var workers sync.WaitGroup
	for i := 0; i < cap(results); i++ {
		workers.Add(1)
		go func(editor int) {
			defer workers.Done()
			<-start
			selected, name := root, "nested/edited"
			if editor%2 == 0 {
				selected, name = nested, "edited"
			}
			_, err := remoteFileOperation(selected, remoteFileRequest{Operation: "replace", Path: name, ExpectedData: base64.StdEncoding.EncodeToString(original), Data: base64.StdEncoding.EncodeToString([]byte(fmt.Sprintf("saved by editor %d", editor)))})
			results <- err
		}(i)
	}
	close(start)
	workers.Wait()
	close(results)
	succeeded := 0
	for err := range results {
		if err == nil {
			succeeded++
		}
	}
	if succeeded != 1 {
		t.Fatalf("%d concurrent saves accepted the same stale version; exactly one must succeed", succeeded)
	}
	entries, err := os.ReadDir(filepath.Join(folder, "nested"))
	if err != nil || len(entries) != 1 {
		t.Fatal("save left temporary files", entries, err)
	}
}
