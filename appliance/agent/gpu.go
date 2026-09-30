package main

import (
	"fmt"
	"os"
	"syscall"

	"golang.org/x/sys/unix"
)

// This is an appliance-local render-only group, not a host Windows account or
// a privileged container. The device cgroup still gates access per container.
const gpuRenderGroup = 65532

func isGPURenderDevice(info os.FileInfo) bool {
	stat, ok := info.Sys().(*syscall.Stat_t)
	return ok && info.Mode()&os.ModeType == os.ModeDevice|os.ModeCharDevice &&
		unix.Major(uint64(stat.Rdev)) == 226 && unix.Minor(uint64(stat.Rdev)) >= 128
}

func gpuAvailable() bool {
	if cudaMode() {
		_, err := os.Stat("/dev/dxg")
		return err == nil
	}
	info, err := os.Lstat(renderNode)
	return err == nil && isGPURenderDevice(info)
}

func gpuContainerArguments(enabled bool) ([]string, error) {
	if cudaMode() {
		return cudaDeviceArguments(enabled)
	}
	if !enabled {
		return nil, nil
	}
	info, err := os.Lstat(renderNode)
	if err != nil || !isGPURenderDevice(info) {
		return nil, fmt.Errorf("shared GPU render device is unavailable")
	}
	// mdev may initially create a root-only node. Grant the render group access
	// inside this dedicated appliance, without chmod 666 or privileged containers.
	if err := os.Chown(renderNode, 0, gpuRenderGroup); err != nil {
		return nil, fmt.Errorf("prepare shared GPU device ownership: %w", err)
	}
	if err := os.Chmod(renderNode, 0660); err != nil {
		return nil, fmt.Errorf("prepare shared GPU device permissions: %w", err)
	}
	return gpuDeviceArguments(), nil
}

func gpuDeviceArguments() []string {
	// A numeric supplementary group works even in images without /etc/group,
	// and allows OCI USER/non-root apps to open the same assigned render node.
	return []string{"--device", renderNode + ":" + renderNode + ":rw", "--group-add", fmt.Sprint(gpuRenderGroup)}
}
