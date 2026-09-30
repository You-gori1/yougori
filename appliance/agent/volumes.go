package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io/fs"
	"net/http"
	"path/filepath"
	"strings"
	"time"
)

type executor func(context.Context, string, ...string) (commandOutput, error)

type volumeEntry struct {
	Name         string   `json:"name"`
	Mountpoint   string   `json:"mountpoint,omitempty"`
	SizeBytes    *int64   `json:"sizeBytes,omitempty"`
	SizeComplete bool     `json:"sizeComplete,omitempty"`
	UsedBy       []string `json:"usedBy"`
}

// Named volumes outlive their containers; this lists and removes them.
func (s *server) volumes(w http.ResponseWriter, r *http.Request) {
	var request struct {
		Action string `json:"action"`
		Name   string `json:"name"`
		Size   bool   `json:"size"`
	}
	if !decodeRequest(w, r, &request) {
		return
	}
	if s.microVM {
		writeError(w, 409, "Named volumes are managed by the container engine")
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Minute)
	defer cancel()
	switch request.Action {
	case "list":
		volumes, err := listVolumes(ctx, request.Size, run)
		if err != nil {
			writeCommandError(w, err)
			return
		}
		writeJSON(w, 200, volumes)
	case "remove":
		if !workloadName.MatchString(request.Name) {
			writeError(w, 400, "Invalid volume name")
			return
		}
		unlock := s.locks.lock("volume:" + request.Name)
		defer unlock()
		status, err := removeVolume(ctx, request.Name, run)
		if err != nil {
			writeError(w, status, err.Error())
			return
		}
		writeJSON(w, 200, map[string]any{"removed": request.Name})
	default:
		writeError(w, 400, "Unsupported volume action")
	}
}

func listVolumes(ctx context.Context, size bool, execute executor) ([]volumeEntry, error) {
	output, err := execute(ctx, "nerdctl", "--namespace", namespace, "volume", "ls", "--format", "{{json .}}")
	if err != nil {
		return nil, err
	}
	usage, err := volumeUsage(ctx, execute)
	if err != nil {
		return nil, err
	}
	volumes := []volumeEntry{}
	for _, line := range strings.Split(strings.TrimSpace(output.Stdout), "\n") {
		if strings.TrimSpace(line) == "" {
			continue
		}
		var raw struct {
			Name       string `json:"Name"`
			Mountpoint string `json:"Mountpoint"`
		}
		if err := json.Unmarshal([]byte(line), &raw); err != nil || raw.Name == "" {
			return nil, fmt.Errorf("cannot decode the volume list")
		}
		entry := volumeEntry{Name: raw.Name, Mountpoint: raw.Mountpoint, UsedBy: usage[raw.Name]}
		if entry.UsedBy == nil {
			entry.UsedBy = []string{}
		}
		if size && raw.Mountpoint != "" {
			total, complete := directorySize(ctx, raw.Mountpoint, 2_000_000)
			entry.SizeBytes, entry.SizeComplete = &total, complete
		}
		volumes = append(volumes, entry)
	}
	return volumes, nil
}

// volumeUsage maps each named volume to the containers (running or stopped) that mount it.
func volumeUsage(ctx context.Context, execute executor) (map[string][]string, error) {
	names, err := execute(ctx, "nerdctl", "--namespace", namespace, "ps", "--all", "--format", "{{.Names}}")
	if err != nil {
		return nil, err
	}
	containers := strings.Fields(names.Stdout)
	usage := map[string][]string{}
	if len(containers) == 0 {
		return usage, nil
	}
	args := append([]string{"--namespace", namespace, "container", "inspect", "--format", "{{.Name}} {{json .Mounts}}"}, containers...)
	inspected, err := execute(ctx, "nerdctl", args...)
	if err != nil {
		return nil, err
	}
	for _, line := range strings.Split(strings.TrimSpace(inspected.Stdout), "\n") {
		name, mounts, found := strings.Cut(strings.TrimSpace(line), " ")
		if !found {
			continue
		}
		var list []struct {
			Type string `json:"Type"`
			Name string `json:"Name"`
		}
		if err := json.Unmarshal([]byte(mounts), &list); err != nil {
			return nil, fmt.Errorf("cannot read the mounts of container %s", strings.TrimPrefix(name, "/"))
		}
		for _, mount := range list {
			if mount.Type == "volume" && mount.Name != "" {
				usage[mount.Name] = append(usage[mount.Name], strings.TrimPrefix(name, "/"))
			}
		}
	}
	return usage, nil
}

// removeVolume refuses while any container, even a stopped one, still mounts the volume.
func removeVolume(ctx context.Context, name string, execute executor) (int, error) {
	volumes, err := listVolumes(ctx, false, execute)
	if err != nil {
		return 500, err
	}
	var found *volumeEntry
	for index := range volumes {
		if volumes[index].Name == name {
			found = &volumes[index]
		}
	}
	if found == nil {
		return 404, fmt.Errorf("no volume named %s", name)
	}
	if len(found.UsedBy) > 0 {
		return 409, fmt.Errorf("volume %s is used by %s; delete those environments first", name, strings.Join(found.UsedBy, ", "))
	}
	if _, err := execute(ctx, "nerdctl", "--namespace", namespace, "volume", "rm", name); err != nil && !commandReportsNotFound(err, name) {
		return 409, err
	}
	remaining, err := listVolumes(ctx, false, execute)
	if err != nil {
		return 500, fmt.Errorf("verify volume removal: %w", err)
	}
	for _, volume := range remaining {
		if volume.Name == name {
			return 409, fmt.Errorf("volume %s is still present; removal did not finish", name)
		}
	}
	return 200, nil
}

// directorySize adds up regular file sizes, stopping after limit entries or when ctx ends.
func directorySize(ctx context.Context, root string, limit int) (int64, bool) {
	var total int64
	entries := 0
	complete := true
	_ = filepath.WalkDir(root, func(_ string, entry fs.DirEntry, err error) error {
		if err != nil {
			complete = false
			return nil
		}
		entries++
		if entries > limit || ctx.Err() != nil {
			complete = false
			return fs.SkipAll
		}
		if entry.Type().IsRegular() {
			if info, err := entry.Info(); err == nil {
				total += info.Size()
			}
		}
		return nil
	})
	return total, complete
}
