package main

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"golang.org/x/sys/unix"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"time"
)

var fabricProcesses sync.Map

func stopFabric(id string) {
	if value, ok := fabricProcesses.LoadAndDelete(id); ok {
		value.(context.CancelFunc)()
	}
}

// Runs in a separate process already inside the target network namespace.
// Closing the nonpersistent TAP removes the interface and its routes.
func fabricTap(address, mac string) error {
	if !validFabricAddress(address, mac) {
		return fmt.Errorf("invalid private network identity")
	}
	fd, err := unix.Open("/dev/net/tun", unix.O_RDWR|unix.O_CLOEXEC, 0)
	if err != nil {
		return err
	}
	tap := os.NewFile(uintptr(fd), "odprivate")
	defer tap.Close()
	ifr, err := unix.NewIfreq("odprivate")
	if err != nil {
		return err
	}
	ifr.SetUint16(unix.IFF_TAP | unix.IFF_NO_PI)
	if err = unix.IoctlIfreq(fd, unix.TUNSETIFF, ifr); err != nil {
		return err
	}
	for _, args := range [][]string{{"link", "set", "odprivate", "address", mac}, {"addr", "replace", address + "/11", "dev", "odprivate"}, {"link", "set", "odprivate", "up"}} {
		if output, err := exec.Command("ip", args...).CombinedOutput(); err != nil {
			return fmt.Errorf("configure private adapter: %s: %w", output, err)
		}
	}
	ready := os.NewFile(3, "private-adapter-ready")
	if ready != nil {
		_, _ = ready.Write([]byte{1})
		ready.Close()
	}
	errors := make(chan error, 2)
	go func() {
		buffer := make([]byte, 65536)
		for {
			n, err := tap.Read(buffer)
			if err != nil {
				errors <- err
				return
			}
			if err = binary.Write(os.Stdout, binary.BigEndian, uint32(n)); err == nil {
				_, err = os.Stdout.Write(buffer[:n])
			}
			if err != nil {
				errors <- err
				return
			}
		}
	}()
	go func() {
		for {
			var n uint32
			if err := binary.Read(os.Stdin, binary.BigEndian, &n); err != nil {
				errors <- err
				return
			}
			if n < 14 || n > 65536 {
				errors <- fmt.Errorf("invalid private packet length")
				return
			}
			packet := make([]byte, n)
			if _, err := io.ReadFull(os.Stdin, packet); err != nil {
				errors <- err
				return
			}
			if _, err := tap.Write(packet); err != nil {
				errors <- err
				return
			}
		}
	}()
	return <-errors
}

func validFabricAddress(address, mac string) bool {
	ip := net.ParseIP(address).To4()
	hardware, err := net.ParseMAC(mac)
	return ip != nil && ip[0] == 10 && ip[1] >= 192 && ip[1] < 224 && ip[3] >= 2 && ip[3] <= 253 && err == nil && len(hardware) == 6 && hardware[0] == 0x52 && hardware[1] == 0x54 && hardware[2] == 0x4f && hardware[3] == ip[1] && hardware[4] == ip[2] && hardware[5] == ip[3]
}

func (s *server) attachFabric(w http.ResponseWriter, r *http.Request) {
	var request struct {
		ID      string `json:"id"`
		Port    uint16 `json:"port"`
		Token   string `json:"token"`
		Address string `json:"address"`
		MAC     string `json:"mac"`
	}
	if !decodeRequest(w, r, &request) || !requireID(w, request.ID) {
		return
	}
	if s.microVM || request.Port == 0 || len(request.Token) != 64 || strings.Trim(request.Token, "0123456789abcdef") != "" || !validFabricAddress(request.Address, request.MAC) {
		writeError(w, 400, "invalid private network configuration")
		return
	}
	unlock := s.locks.lock(containerLockKey(request.ID))
	defer unlock()
	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	defer cancel()
	pid, err := containerPID(ctx, request.ID)
	if err != nil {
		writeError(w, 409, "container is not running")
		return
	}
	connection, err := net.DialTimeout("tcp", net.JoinHostPort("10.0.2.2", strconv.Itoa(int(request.Port))), 3*time.Second)
	if err != nil {
		writeError(w, 500, "connect private network: "+err.Error())
		return
	}
	connection.SetWriteDeadline(time.Now().Add(3 * time.Second))
	if _, err = io.WriteString(connection, request.Token); err != nil {
		connection.Close()
		writeError(w, 500, "authenticate private network")
		return
	}
	connection.SetWriteDeadline(time.Time{})
	stopFabric(request.ID)
	lifetime, stop := context.WithCancel(context.Background())
	executable, err := os.Executable()
	if err != nil {
		stop()
		connection.Close()
		writeError(w, 500, err.Error())
		return
	}
	_ = exec.Command("modprobe", "tun").Run()
	if _, err := os.Stat("/dev/net/tun"); os.IsNotExist(err) {
		_ = os.MkdirAll("/dev/net", 0755)
		_ = unix.Mknod("/dev/net/tun", unix.S_IFCHR|0600, int(unix.Mkdev(10, 200)))
	}
	command := exec.CommandContext(lifetime, "nsenter", "-t", strconv.Itoa(pid), "-n", "--", executable, "fabric-tap", request.Address, request.MAC)
	socket, err := connection.(*net.TCPConn).File()
	if err != nil {
		stop()
		connection.Close()
		writeError(w, 500, err.Error())
		return
	}
	defer socket.Close()
	ready, readyWriter, err := os.Pipe()
	if err != nil {
		stop()
		connection.Close()
		writeError(w, 500, err.Error())
		return
	}
	defer ready.Close()
	command.Stdin = socket
	command.Stdout = socket
	var helperError bytes.Buffer
	command.Stderr = &helperError
	command.ExtraFiles = []*os.File{readyWriter}
	if err = command.Start(); err != nil {
		readyWriter.Close()
		stop()
		connection.Close()
		writeError(w, 500, err.Error())
		return
	}
	readyWriter.Close()
	ready.SetReadDeadline(time.Now().Add(3 * time.Second))
	var signal [1]byte
	if _, err = io.ReadFull(ready, signal[:]); err != nil || signal[0] != 1 {
		stop()
		connection.Close()
		_ = command.Wait()
		writeError(w, 500, "could not configure the container private adapter: "+strings.TrimSpace(helperError.String()))
		return
	}
	fabricProcesses.Store(request.ID, context.CancelFunc(func() { connection.Close(); stop() }))
	go func() { defer connection.Close(); defer stop(); _ = command.Wait() }()
	writeJSON(w, 200, map[string]bool{"connected": true})
}

// The built-in MicroVM has no network manager for additional NICs. Identify our
// adapter by its dedicated MAC prefix, not by potentially reordered eth numbers.
func configureMicroVMFabric() error {
	interfaces, err := net.Interfaces()
	if err != nil {
		return err
	}
	for _, nic := range interfaces {
		mac := nic.HardwareAddr
		if len(mac) != 6 || mac[0] != 0x52 || mac[1] != 0x54 || mac[2] != 0x4f {
			continue
		}
		address := fmt.Sprintf("10.%d.%d.%d", mac[3], mac[4], mac[5])
		if !validFabricAddress(address, mac.String()) {
			continue
		}
		for _, args := range [][]string{{"addr", "replace", address + "/11", "dev", nic.Name}, {"link", "set", nic.Name, "up"}} {
			if output, err := exec.Command("ip", args...).CombinedOutput(); err != nil {
				return fmt.Errorf("configure private adapter: %s: %w", output, err)
			}
		}
	}
	return nil
}
