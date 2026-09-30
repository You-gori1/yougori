package main

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"strings"
	"syscall"
	"time"
	"unsafe"
)

// Only the private runtime root is trimmed. FITRIM asks the filesystem for free
// extents; it never zeros occupied files, guest partitions, or host shares.
func trimRootStorage() error {
	if _, err := containerStoreDevice(); err == nil {
		if err := trimStoragePath(containerStoreMount); err != nil {
			return err
		}
	}
	return trimStoragePath("/")
}

func trimStoragePath(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	syscall.Sync()
	extent := [3]uint64{0, ^uint64(0), 1024 * 1024}
	_, _, errno := syscall.Syscall(syscall.SYS_IOCTL, file.Fd(), 0xc0185879, uintptr(unsafe.Pointer(&extent[0])))
	if errno != 0 {
		return fmt.Errorf("return free runtime blocks: %w", errno)
	}
	return nil
}

type storageReclaimResult struct {
	Busy     bool     `json:"busy"`
	Warnings []string `json:"warnings"`
}

// No image/volume prune: cached base images and snapshot tags may still be
// needed for offline reset or restore, including stopped containers.
func reclaimRuntimeStorage(ctx context.Context, execute func(context.Context, string, ...string) (commandOutput, error), executable string) storageReclaimResult {
	result := storageReclaimResult{Busy: true, Warnings: []string{}}
	running, err := execute(ctx, "nerdctl", "--namespace", namespace, "ps", "--quiet")
	if err != nil {
		result.Warnings = append(result.Warnings, "Cannot verify idle containers; offline disk compaction was skipped.")
	} else {
		result.Busy = strings.TrimSpace(running.Stdout) != ""
	}
	if _, err := execute(ctx, executable, "--trim-storage"); err != nil {
		result.Warnings = append(result.Warnings, "Free blocks remain inside the container disk: "+err.Error())
	}
	return result
}

func (s *server) reclaimStorage(w http.ResponseWriter, r *http.Request) {
	if s.microVM {
		writeError(w, http.StatusConflict, "Shared storage cleanup is only available in the container runtime")
		return
	}
	unlock := s.locks.lock("storage-reclaim")
	defer unlock()
	executable, err := os.Executable()
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 90*time.Second)
	defer cancel()
	writeJSON(w, http.StatusOK, reclaimRuntimeStorage(ctx, run, executable))
}
