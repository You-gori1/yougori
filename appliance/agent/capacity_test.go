package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestHotplugRequiresEveryExpectedBlockOnline(t *testing.T) {
	root := t.TempDir()
	write := func(path, value string) {
		t.Helper()
		if err := os.MkdirAll(filepath.Dir(filepath.Join(root, path)), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, path), []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
	}
	write("block_size_bytes", "8000000\n")
	ranges := []memoryRange{{Start: 128 * 1024 * 1024, Size: 256 * 1024 * 1024}}
	write("memory1/online", "1\n")
	if ready, err := memoryRangesOnline(root, ranges); err != nil || ready {
		t.Fatalf("missing block: %v %v", ready, err)
	}
	write("memory2/online", "0\n")
	if ready, err := memoryRangesOnline(root, ranges); err != nil || ready {
		t.Fatalf("offline block: %v %v", ready, err)
	}
	write("memory2/online", "1\n")
	if ready, err := memoryRangesOnline(root, ranges); err != nil || !ready {
		t.Fatalf("online blocks: %v %v", ready, err)
	}
	for _, invalid := range []memoryRange{{Start: 1, Size: 128 * 1024 * 1024}, {Size: 1}, {Start: ^uint64(0), Size: 128 * 1024 * 1024}, {Size: 0}} {
		if _, err := memoryRangesOnline(root, []memoryRange{invalid}); err == nil {
			t.Fatalf("accepted %v", invalid)
		}
	}
}

func TestCapacityMemoryUsesRealGuestTotal(t *testing.T) {
	value, err := capacityMemoryBytes("MemTotal:        8576000 kB\nMemFree: 100 kB\n")
	if err != nil || value != 8576000*1024 {
		t.Fatalf("memory=%d, error=%v", value, err)
	}
	for _, input := range []string{"", "MemFree: 42 kB", "MemTotal: 0 kB", "MemTotal: 5 GB", "MemTotal: 18446744073709551615 kB"} {
		if _, err := capacityMemoryBytes(input); err == nil {
			t.Fatalf("accepted invalid capacity %q", input)
		}
	}
}
