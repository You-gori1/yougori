package main

import (
	"context"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"os/exec"
	"time"
)

const firmwareTokenPath = "/sys/firmware/qemu_fw_cfg/by_name/opt/yougori/control-token/raw"

func firmwareProbeTimeout(emulated bool) time.Duration {
	if emulated {
		return 2 * time.Minute
	}
	return 5 * time.Second
}

func readFirmwareBootToken(emulated bool) (string, error) {
	// Alpine may ship this driver as a module. It exposes firmware to the host
	// guest agent; the OCI runtime masks /sys/firmware inside containers.
	if _, err := os.Stat(firmwareTokenPath); os.IsNotExist(err) {
		ctx, cancel := context.WithTimeout(context.Background(), firmwareProbeTimeout(emulated))
		defer cancel()
		_ = exec.CommandContext(ctx, "modprobe", "qemu_fw_cfg").Run()
	}
	return loadFirmwareBootToken(firmwareTokenPath)
}

func loadFirmwareBootToken(path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", fmt.Errorf("private firmware credential unavailable")
	}
	defer file.Close()
	value, err := io.ReadAll(io.LimitReader(file, 65))
	if err != nil || len(value) != 64 {
		return "", fmt.Errorf("invalid private firmware credential")
	}
	if _, err := hex.DecodeString(string(value)); err != nil {
		return "", fmt.Errorf("invalid private firmware credential")
	}
	return string(value), nil
}
