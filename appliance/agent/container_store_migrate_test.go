package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestStorageDestinationAcceptsEmptyNerdctlScaffold(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nerdctl")
	if err := os.MkdirAll(filepath.Join(path, "filesystem-ops"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := requireEmptyStorageDirectory(path); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(path)
	if err != nil || len(entries) != 0 {
		t.Fatalf("destination not empty: %v, %v", entries, err)
	}
	if err := requireEmptyStorageDirectory(path); err != nil {
		t.Fatal("retry failed:", err)
	}
}

func TestStorageDestinationPreservesUnexpectedData(t *testing.T) {
	for _, kind := range []string{"file", "nonempty", "symlink", "unknown", "other-tree"} {
		t.Run(kind, func(t *testing.T) {
			root := t.TempDir()
			path := filepath.Join(root, "nerdctl")
			if kind == "other-tree" {
				path = filepath.Join(root, "containerd")
			}
			if err := os.Mkdir(path, 0700); err != nil {
				t.Fatal(err)
			}
			scaffold := filepath.Join(path, "filesystem-ops")
			switch kind {
			case "file":
				if err := os.WriteFile(scaffold, []byte("keep"), 0600); err != nil {
					t.Fatal(err)
				}
			case "symlink":
				if err := os.Symlink(root, scaffold); err != nil {
					t.Fatal(err)
				}
			default:
				if err := os.Mkdir(scaffold, 0700); err != nil {
					t.Fatal(err)
				}
				if kind == "nonempty" {
					if err := os.WriteFile(filepath.Join(scaffold, "keep"), []byte("keep"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				if kind == "unknown" {
					if err := os.Mkdir(filepath.Join(path, "unknown"), 0700); err != nil {
						t.Fatal(err)
					}
				}
			}
			if err := requireEmptyStorageDirectory(path); err == nil {
				t.Fatal("unexpected data accepted")
			}
			if _, err := os.Lstat(scaffold); err != nil {
				t.Fatal("existing entry was removed:", err)
			}
			if kind == "nonempty" {
				if contents, err := os.ReadFile(filepath.Join(scaffold, "keep")); err != nil || string(contents) != "keep" {
					t.Fatal("existing data changed")
				}
			}
		})
	}
}
