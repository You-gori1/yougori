package main

import (
	"os"
	"reflect"
	"syscall"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

type gpuFileInfo struct {
	mode os.FileMode
	stat syscall.Stat_t
}

func (i gpuFileInfo) Name() string       { return "renderD128" }
func (i gpuFileInfo) Size() int64        { return 0 }
func (i gpuFileInfo) Mode() os.FileMode  { return i.mode }
func (i gpuFileInfo) ModTime() time.Time { return time.Time{} }
func (i gpuFileInfo) IsDir() bool        { return false }
func (i gpuFileInfo) Sys() interface{}   { return &i.stat }

func TestGPURenderDeviceIsNotJustAnyDeviceOrSymlink(t *testing.T) {
	valid := gpuFileInfo{mode: os.ModeDevice | os.ModeCharDevice | 0660, stat: syscall.Stat_t{Rdev: unix.Mkdev(226, 128)}}
	if !isGPURenderDevice(valid) {
		t.Fatal("DRM render node should be accepted")
	}
	for _, mode := range []os.FileMode{os.ModeDevice, os.ModeSymlink, os.ModeDir, 0660} {
		other := valid
		other.mode = mode
		if isGPURenderDevice(other) {
			t.Fatalf("accepted non-render device mode %v", mode)
		}
	}
	for _, device := range []uint64{unix.Mkdev(226, 0), unix.Mkdev(1, 3), unix.Mkdev(8, 0)} {
		other := valid
		other.stat.Rdev = device
		if isGPURenderDevice(other) {
			t.Fatalf("accepted non-render device %d", device)
		}
	}
}

func TestGPUAccessUsesOnlyTheRenderNodeAndAnUnprivilegedSupplementaryGroup(t *testing.T) {
	want := []string{"--device", "/dev/dri/renderD128:/dev/dri/renderD128:rw", "--group-add", "65532"}
	if !reflect.DeepEqual(gpuDeviceArguments(), want) {
		t.Fatal(gpuDeviceArguments())
	}
	args, err := gpuContainerArguments(false)
	if err != nil || len(args) != 0 {
		t.Fatalf("disconnected GPU must not inspect or expose any device: %v %v", args, err)
	}
}
