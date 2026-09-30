package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"testing"

	"golang.org/x/sys/unix"
)

func TestStorageMigrationCopyPreservesFilesAndMetadata(t *testing.T) {
	root := t.TempDir()
	source, destination := filepath.Join(root, "source"), filepath.Join(root, "copy")
	if err := os.Mkdir(source, 0750); err != nil {
		t.Fatal(err)
	}
	sparse := filepath.Join(source, "sparse")
	file, err := os.Create(sparse)
	if err != nil {
		t.Fatal(err)
	}
	if err := file.Truncate(16 << 20); err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteAt([]byte("preserved extent"), 8<<20); err != nil {
		t.Fatal(err)
	}
	file.Close()
	if err := os.Chmod(sparse, 0640); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(sparse, filepath.Join(source, "hardlink")); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(root, "outside")
	if err := os.WriteFile(outside, []byte("host data stays here"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(source, "symlink")); err != nil {
		t.Fatal(err)
	}
	if err := unix.Mkfifo(filepath.Join(source, "fifo"), 0600); err != nil {
		t.Fatal(err)
	}
	attribute := unix.Setxattr(sparse, "user.migration-test", []byte("kept"), 0)
	if attribute != nil && !errors.Is(attribute, unix.ENOTSUP) {
		t.Fatal(attribute)
	}
	// Cross the directory reader's batch boundary without loading a manifest.
	for i := 0; i < 257; i++ {
		if err := os.WriteFile(filepath.Join(source, fmt.Sprintf("entry-%03d", i)), []byte("kept"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	before, err := os.Stat(sparse)
	if err != nil {
		t.Fatal(err)
	}
	if err := copyContainerStorage(context.Background(), source, destination); err != nil {
		t.Fatal(err)
	}
	copied := filepath.Join(destination, "sparse")
	after, err := os.Stat(copied)
	if err != nil {
		t.Fatal(err)
	}
	if after.Size() != before.Size() || after.Mode() != before.Mode() || !after.ModTime().Equal(before.ModTime()) {
		t.Fatal("file metadata changed")
	}
	if after.Sys().(*syscall.Stat_t).Blocks > 128 {
		t.Fatal("sparse file was expanded into occupied storage")
	}
	hardlink, err := os.Stat(filepath.Join(destination, "hardlink"))
	if err != nil || !os.SameFile(after, hardlink) {
		t.Fatal("hardlink was not preserved")
	}
	read, err := os.Open(copied)
	if err != nil {
		t.Fatal(err)
	}
	got := make([]byte, len("preserved extent"))
	_, err = read.ReadAt(got, 8<<20)
	read.Close()
	if err != nil || string(got) != "preserved extent" {
		t.Fatal("copied data changed")
	}
	target, err := os.Readlink(filepath.Join(destination, "symlink"))
	if err != nil || target != outside {
		t.Fatal("symlink was followed or altered")
	}
	if got, err := os.ReadFile(outside); err != nil || string(got) != "host data stays here" {
		t.Fatal("external file was touched")
	}
	if attribute == nil {
		got := make([]byte, 16)
		n, err := unix.Getxattr(copied, "user.migration-test", got)
		if err != nil || string(got[:n]) != "kept" {
			t.Fatal("extended attribute was lost")
		}
	}
	if got, err := os.ReadDir(destination); err != nil || len(got) != 261 {
		t.Fatal("not every directory batch was copied")
	}
	if _, err := os.Lstat(filepath.Join(source, "fifo")); err != nil {
		t.Fatal("original disappeared")
	}
}

func TestStorageMigrationCopyNeverReplacesExistingFilesOrRemovesOriginals(t *testing.T) {
	root := t.TempDir()
	source, destination := filepath.Join(root, "source"), filepath.Join(root, "existing")
	if err := os.Mkdir(source, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(destination, 0700); err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{source, destination} {
		if err := os.WriteFile(filepath.Join(dir, "keep"), []byte(dir), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := copyContainerStorage(context.Background(), source, destination); !errors.Is(err, os.ErrExist) {
		t.Fatalf("unexpected overwrite result: %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := copyContainerStorage(ctx, source, filepath.Join(root, "cancelled")); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled copy: %v", err)
	}
	for _, dir := range []string{source, destination} {
		if got, err := os.ReadFile(filepath.Join(dir, "keep")); err != nil || string(got) != dir {
			t.Fatal("original data was changed")
		}
	}
}

func TestStorageUsageCountsHardlinksOnceAndDoesNotFollowSymlinks(t *testing.T) {
	root := t.TempDir()
	file := filepath.Join(root, "file")
	if err := os.WriteFile(file, make([]byte, 8192), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(file, filepath.Join(root, "link")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink("/", filepath.Join(root, "outside")); err != nil {
		t.Fatal(err)
	}
	used, err := allocatedStorageBytes([]string{root, root})
	if err != nil || used < 8192 || used > 16384 {
		t.Fatalf("incorrect allocation: %d, %v", used, err)
	}
}
