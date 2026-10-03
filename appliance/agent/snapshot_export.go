package main

import (
	"compress/gzip"
	"context"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

const snapshotStreamMagic = "YOUGORI-SNAPSHOT\x00\x01"

// The payload is small length-prefixed image metadata followed by one gzip
// rootfs stream. Closing gzip is the success marker: failed/cancelled exports
// never get a valid footer, so the desktop cannot accept a partial snapshot.
func writeSnapshotStream(w io.Writer, metadata []byte, export func(io.Writer) error) error {
	if len(metadata) == 0 || len(metadata) > 1024*1024 {
		return fmt.Errorf("snapshot configuration exceeds 1 MiB")
	}
	if _, err := io.WriteString(w, snapshotStreamMagic); err != nil {
		return err
	}
	if err := binary.Write(w, binary.BigEndian, uint32(len(metadata))); err != nil {
		return err
	}
	if _, err := w.Write(metadata); err != nil {
		return err
	}
	compressed, err := gzip.NewWriterLevel(w, gzip.BestSpeed)
	if err != nil {
		return err
	}
	if err = export(compressed); err != nil {
		return err
	}
	return compressed.Close()
}

// nerdctl export reads the merged rootfs and streams it without committing a
// second writable layer or saving a tar inside the shared container disk.
func (s *server) exportSnapshot(w http.ResponseWriter, r *http.Request) {
	s.exportSnapshotWithConfiguration(w, r, snapshotConfiguration)
}

func (s *server) exportSnapshotWithConfiguration(w http.ResponseWriter, r *http.Request, configure func(context.Context, string, json.RawMessage, json.RawMessage) (json.RawMessage, error)) {
	var request snapshotRequest
	if !decodeRequest(w, r, &request) || !requireID(w, request.ID) || !requireID(w, request.SnapshotID) {
		return
	}
	ctx, stream, finish, err := s.beginSnapshotExport(r.Context(), w, request.ID, request.SnapshotID)
	if err != nil {
		writeError(w, http.StatusConflict, err.Error())
		return
	}
	defer finish()
	unlock := s.locks.lock(containerLockKey(request.ID), snapshotLockKey(request.SnapshotID))
	defer unlock()
	if err := ctx.Err(); err != nil {
		writeError(w, http.StatusConflict, "snapshot export cancelled before preparation")
		return
	}
	output, err := run(ctx, "nerdctl", "--namespace", namespace, "inspect", "--format", "{{json .}}", request.ID)
	if err != nil {
		writeCommandError(w, err)
		return
	}
	var container struct {
		ID     string `json:"Id"`
		Config json.RawMessage
		Image  string
		State  containerState
		Mounts []snapshotVolume
	}
	if err = json.Unmarshal([]byte(output.Stdout), &container); err != nil || len(container.Config) == 0 || container.Image == "" {
		writeError(w, 500, "Cannot read snapshot configuration")
		return
	}
	volumes, err := privateSnapshotVolumes(container.Mounts)
	if err != nil {
		writeError(w, 500, "Cannot include private container volumes: "+err.Error())
		return
	}
	output, err = run(ctx, "nerdctl", "--namespace", namespace, "image", "inspect", "--format", "{{json .}}", container.Image)
	if err != nil {
		writeCommandError(w, err)
		return
	}
	var platform struct {
		Architecture, Os, Variant string
		Config                    json.RawMessage
	}
	if err = json.Unmarshal([]byte(output.Stdout), &platform); err != nil || platform.Architecture == "" || platform.Os == "" {
		writeError(w, 500, "Cannot read snapshot platform")
		return
	}
	configuration, err := configure(ctx, container.ID, platform.Config, container.Config)
	if err != nil {
		writeError(w, 500, "Cannot preserve container startup configuration: "+err.Error())
		return
	}
	metadata, err := json.Marshal(map[string]any{
		"architecture": platform.Architecture, "os": platform.Os, "variant": platform.Variant,
		"config": configuration, "created": time.Now().UTC().Format(time.RFC3339Nano),
	})
	if err != nil || len(metadata) > 1024*1024 {
		writeError(w, 500, "Snapshot configuration exceeds 1 MiB")
		return
	}
	// /run is runtime-owned RAM. Even a nearly full container disk can export.
	temporary, err := os.MkdirTemp("/run", "yougori-snapshot-")
	if err != nil {
		writeError(w, 500, "Cannot prepare snapshot export: "+err.Error())
		return
	}
	defer cleanupSnapshotMounts(temporary)
	resume := func() error { return nil }
	if container.State.Running && !container.State.Paused {
		if _, err = run(ctx, "nerdctl", "--namespace", namespace, "pause", request.ID); err != nil {
			writeCommandError(w, err)
			return
		}
		resumed := false
		resume = func() error {
			if resumed {
				return nil
			}
			cleanup, cancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cancel()
			_, err := run(cleanup, "nerdctl", "--namespace", namespace, "unpause", request.ID)
			if err == nil {
				resumed = true
			}
			return err
		}
		defer func() {
			if err := resume(); err != nil {
				log.Printf("snapshot %s: resume container: %v", request.SnapshotID, err)
			}
		}()
	}
	syscall.Sync()
	w.Header().Set("Content-Type", "application/vnd.yougori.snapshot.v1")
	err = writeSnapshotStream(stream, metadata, func(out io.Writer) error {
		command := exec.CommandContext(ctx, "nerdctl", "--namespace", namespace, "export", request.ID)
		command.Env = append(os.Environ(), "TMPDIR="+temporary)
		reader, err := command.StdoutPipe()
		if err != nil {
			return err
		}
		var tail logTail
		command.Stderr = &tail
		command.WaitDelay = 3 * time.Second
		if err := command.Start(); err != nil {
			return err
		}
		// Killing the exporter must also unblock its reader if a descendant
		// retained the pipe, so cancellation can reach bounded resume cleanup.
		reading := make(chan struct{})
		go func() {
			select {
			case <-ctx.Done():
				_ = reader.Close()
			case <-reading:
			}
		}()
		merged := mergeSnapshotVolumes(out, reader, volumes)
		close(reading)
		if merged != nil {
			_ = command.Process.Kill()
		}
		if err := command.Wait(); err != nil {
			return fmt.Errorf("export container: %w: %s", err, strings.TrimSpace(string(tail.data)))
		}
		if merged != nil {
			return merged
		}
		return resume()
	})
	if err != nil {
		log.Printf("snapshot %s: %v", request.SnapshotID, err)
	}
}

// A killed exporter may leave its temporary mount behind. Unmount only paths
// inside this operation's private /run directory; never recursively delete a
// mounted container filesystem. Successful nerdctl exports clean themselves up.
func cleanupSnapshotMounts(root string) {
	entries, _ := os.ReadDir(root)
	for _, entry := range entries {
		if entry.IsDir() && strings.HasPrefix(entry.Name(), "nerdctl-export-") {
			path := filepath.Join(root, entry.Name())
			_ = syscall.Unmount(path, syscall.MNT_DETACH)
			_ = os.Remove(path)
		}
	}
	_ = os.Remove(root)
}
