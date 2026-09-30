package main

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"strconv"
	"time"
)

// Unlike the QEMU socket path, this stream is initiated by the host and also
// works with WSL NAT. The runtime bearer token authenticates it before upgrade.
func (s *server) fabricStream(w http.ResponseWriter, r *http.Request) {
	var request struct {
		ID      string `json:"id"`
		Address string `json:"address"`
		MAC     string `json:"mac"`
	}
	if !decodeRequest(w, r, &request) || !requireID(w, request.ID) {
		return
	}
	if s.microVM || !validFabricAddress(request.Address, request.MAC) {
		writeError(w, 400, "invalid private adapter")
		return
	}
	unlock := s.locks.lock(containerLockKey(request.ID))
	unlocked := false
	defer func() {
		if !unlocked {
			unlock()
		}
	}()
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	pid, err := containerPID(ctx, request.ID)
	cancel()
	if err != nil {
		writeError(w, 409, "container is not running")
		return
	}
	executable, err := os.Executable()
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	lifetime, stop := context.WithCancel(context.Background())
	defer stop()
	command := exec.CommandContext(lifetime, "nsenter", "-t", strconv.Itoa(pid), "-n", "--", executable, "fabric-tap", request.Address, request.MAC)
	input, err := command.StdinPipe()
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	defer input.Close()
	output, err := command.StdoutPipe()
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	defer output.Close()
	ready, readyWriter, err := os.Pipe()
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	defer ready.Close()
	defer readyWriter.Close()
	command.ExtraFiles = []*os.File{readyWriter}
	stopFabric(request.ID)
	if err = command.Start(); err != nil {
		writeError(w, 500, err.Error())
		return
	}
	readyWriter.Close()
	defer func() { stop(); command.Wait() }()
	ready.SetReadDeadline(time.Now().Add(4 * time.Second))
	var signal [1]byte
	if _, err = io.ReadFull(ready, signal[:]); err != nil {
		writeError(w, 500, "private network adapter did not initialize")
		return
	}
	hijacker, ok := w.(http.Hijacker)
	if !ok {
		writeError(w, 500, "streaming unavailable")
		return
	}
	connection, buffer, err := hijacker.Hijack()
	if err != nil {
		return
	}
	defer connection.Close()
	fmt.Fprint(buffer, "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: yougori-fabric\r\n\r\n")
	if buffer.Flush() != nil {
		return
	}
	fabricProcesses.Store(request.ID, stop)
	unlock()
	unlocked = true
	errors := make(chan error, 2)
	go func() { _, err := io.Copy(input, buffer); errors <- err }()
	go func() { _, err := io.Copy(connection, output); errors <- err }()
	<-errors
}
