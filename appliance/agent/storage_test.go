package main

import (
	"encoding/binary"
	"testing"
)

func TestExt4Capacity(t *testing.T) {
	sb := make([]byte, 1024)
	if _, err := ext4Capacity(sb); err == nil {
		t.Fatal("invalid magic accepted")
	}
	binary.LittleEndian.PutUint16(sb[0x38:], 0xef53)
	binary.LittleEndian.PutUint32(sb[0x18:], 2)
	binary.LittleEndian.PutUint32(sb[4:], 1572864)
	if size, err := ext4Capacity(sb); err != nil || size != 6*1024*1024*1024 {
		t.Fatalf("size=%d error=%v", size, err)
	}
	binary.LittleEndian.PutUint32(sb[0x150:], 1)
	if size, _ := ext4Capacity(sb); size != 6*1024*1024*1024 {
		t.Fatal("high bits without 64bit feature were used")
	}
	binary.LittleEndian.PutUint32(sb[0x60:], 0x80)
	if size, _ := ext4Capacity(sb); size != ((1<<32)+1572864)*4096 {
		t.Fatal("64bit size not recognized")
	}
	binary.LittleEndian.PutUint32(sb[0x18:], 63)
	if _, err := ext4Capacity(sb); err == nil {
		t.Fatal("invalid block size accepted")
	}
	if _, err := ext4Capacity(nil); err == nil {
		t.Fatal("short superblock accepted")
	}
}
