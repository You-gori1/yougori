package main

import (
	"archive/tar"
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func importArchive(t *testing.T, headers ...*tar.Header) []byte {
	t.Helper()
	var buffer bytes.Buffer
	writer := tar.NewWriter(&buffer)
	for _, header := range headers {
		if err := writer.WriteHeader(header); err != nil {
			t.Fatal(err)
		}
		if header.Size > 0 {
			if _, err := writer.Write(bytes.Repeat([]byte{0x42}, int(header.Size))); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	return buffer.Bytes()
}

func TestFileImportNestedFoldersBinaryAndEmptyFiles(t *testing.T) {
	root := t.TempDir()
	archive := importArchive(t,
		&tar.Header{Name: "project ü", Typeflag: tar.TypeDir},
		&tar.Header{Name: "project ü/empty", Typeflag: tar.TypeDir},
		&tar.Header{Name: "project ü/data.bin", Typeflag: tar.TypeReg, Size: 300000, Mode: 06777},
		&tar.Header{Name: "empty.txt", Typeflag: tar.TypeReg},
	)
	size, err := unpackImport(bytes.NewReader(archive), root)
	if err != nil || size != 300000 {
		t.Fatalf("size=%d error=%v", size, err)
	}
	data, err := os.ReadFile(filepath.Join(root, "project ü/data.bin"))
	if err != nil || !bytes.Equal(data, bytes.Repeat([]byte{0x42}, 300000)) {
		t.Fatal("copied content differs", err)
	}
	info, _ := os.Stat(filepath.Join(root, "project ü/data.bin"))
	if info.Mode().Perm() != 0755 || info.Mode()&os.ModeSetuid != 0 {
		t.Fatal("unsafe permissions", info.Mode())
	}
	if _, err := unpackImport(bytes.NewReader(archive), root); err == nil {
		t.Fatal("overwrote an existing copy")
	}
}

func TestFileImportPreservesPrivateFilesAndDirectoryVisibility(t *testing.T) {
	root := t.TempDir()
	archive := importArchive(t,
		&tar.Header{Name: ".ssh", Typeflag: tar.TypeDir, Mode: 0700},
		&tar.Header{Name: ".ssh/private-key", Typeflag: tar.TypeReg, Mode: 0600, Size: 8},
		&tar.Header{Name: "private-tool", Typeflag: tar.TypeReg, Mode: 0700, Size: 3},
		&tar.Header{Name: "public", Typeflag: tar.TypeDir, Mode: 0755},
		&tar.Header{Name: "public/data", Typeflag: tar.TypeReg, Mode: 0644, Size: 2},
	)
	if size, err := unpackImport(bytes.NewReader(archive), root); err != nil || size != 13 {
		t.Fatalf("size=%d error=%v", size, err)
	}
	for name, want := range map[string]os.FileMode{
		".ssh": 0700, ".ssh/private-key": 0600, "private-tool": 0700,
		"public": 0755, "public/data": 0644,
	} {
		info, err := os.Stat(filepath.Join(root, name))
		if err != nil {
			t.Fatal(err)
		}
		if got := info.Mode().Perm(); got != want {
			t.Errorf("%s: permissions %04o, want %04o", name, got, want)
		}
	}
	data, err := os.ReadFile(filepath.Join(root, ".ssh/private-key"))
	if err != nil || !bytes.Equal(data, bytes.Repeat([]byte{0x42}, 8)) {
		t.Fatal("private copied content differs", err)
	}
}

func TestFileImportRejectsUnsafeArchives(t *testing.T) {
	for _, name := range []string{"../outside", "/absolute", "a/../../outside", "a\\outside", "C:outside", "a/./b", "."} {
		t.Run(name, func(t *testing.T) {
			root := t.TempDir()
			archive := importArchive(t, &tar.Header{Name: name, Typeflag: tar.TypeReg})
			if _, err := unpackImport(bytes.NewReader(archive), root); err == nil {
				t.Fatal("accepted unsafe path")
			}
		})
	}
	for _, kind := range []byte{tar.TypeSymlink, tar.TypeLink, tar.TypeChar, tar.TypeBlock, tar.TypeFifo} {
		archive := importArchive(t, &tar.Header{Name: "link", Typeflag: kind, Linkname: "/outside"})
		if _, err := unpackImport(bytes.NewReader(archive), t.TempDir()); err == nil {
			t.Fatalf("accepted entry type %v", kind)
		}
	}
	for _, kind := range []byte{tar.TypeReg, tar.TypeDir} {
		archive := importArchive(t, &tar.Header{Name: "duplicate", Typeflag: kind}, &tar.Header{Name: "duplicate", Typeflag: kind})
		if _, err := unpackImport(bytes.NewReader(archive), t.TempDir()); err == nil {
			t.Fatal("accepted a repeated archive entry")
		}
	}
}

func TestFileImportRejectsTruncatedDataAndExistingParentLinks(t *testing.T) {
	archive := importArchive(t, &tar.Header{Name: "file", Typeflag: tar.TypeReg, Size: 4096})
	if _, err := unpackImport(io.LimitReader(bytes.NewReader(archive), 700), t.TempDir()); err == nil {
		t.Fatal("accepted partial data")
	}
	root, outside := t.TempDir(), t.TempDir()
	if err := os.Mkdir(filepath.Join(outside, "nested"), 0755); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(root, "escape")); err != nil {
		t.Fatal(err)
	}
	archive = importArchive(t, &tar.Header{Name: "escape/file", Typeflag: tar.TypeReg, Size: 1})
	if _, err := unpackImport(bytes.NewReader(archive), root); err == nil {
		t.Fatal("followed existing parent link")
	}
	archive = importArchive(t, &tar.Header{Name: "escape/nested/file", Typeflag: tar.TypeReg, Size: 1})
	if _, err := unpackImport(bytes.NewReader(archive), root); err == nil {
		t.Fatal("followed ancestor link")
	}
	entries, _ := os.ReadDir(outside)
	if len(entries) != 1 {
		t.Fatal("wrote outside copy destination")
	}
	entries, _ = os.ReadDir(filepath.Join(outside, "nested"))
	if len(entries) != 0 {
		t.Fatal("wrote through ancestor link")
	}
}

func TestFileImportRequiresCompleteArchiveTerminatorAndBody(t *testing.T) {
	archive := importArchive(t, &tar.Header{Name: "file", Typeflag: tar.TypeReg, Size: 4096})
	for _, fixture := range []struct {
		name string
		data []byte
	}{
		{"empty request", nil},
		{"missing terminator after complete file", archive[:len(archive)-1024]},
		{"only one terminator block", archive[:len(archive)-512]},
		{"nonzero trailing bytes", append(append([]byte(nil), archive...), []byte("unexpected archive payload")...)},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			if _, err := unpackImport(bytes.NewReader(fixture.data), t.TempDir()); err == nil {
				t.Fatal("accepted an incomplete or ambiguous archive body")
			}
		})
	}
	// Conventional tar record padding remains valid, and must be consumed.
	padded := append(append([]byte(nil), archive...), make([]byte, 1024)...)
	reader := bytes.NewReader(padded)
	if size, err := unpackImport(reader, t.TempDir()); err != nil || size != 4096 || reader.Len() != 0 {
		t.Fatalf("valid padded archive was not fully consumed: size=%d remaining=%d err=%v", size, reader.Len(), err)
	}
}

func TestFileImportExtendedMetadataCannotImpersonateTerminator(t *testing.T) {
	// A complete first file followed by an orphan extended header previously
	// looked complete because Next consumed more than 1024 metadata bytes.
	base := importArchive(t, &tar.Header{Name: "file", Typeflag: tar.TypeReg})
	base = base[:len(base)-1024]
	metadata := func(kind byte, payload []byte) []byte {
		header := append([]byte(nil), base[:512]...)
		header[156] = kind
		copy(header[124:136], fmt.Sprintf("%011o\x00", len(payload)))
		for i := 148; i < 156; i++ {
			header[i] = ' '
		}
		sum := 0
		for _, value := range header {
			sum += int(value)
		}
		copy(header[148:156], fmt.Sprintf("%06o\x00 ", sum))
		result := append(header, payload...)
		return append(result, make([]byte, (512-len(payload)%512)%512)...)
	}
	for _, fixture := range []struct {
		name string
		data []byte
	}{
		{"PAX", metadata(tar.TypeXHeader, []byte("20 path=orphan-name\n"))},
		{"GNU long name", metadata(tar.TypeGNULongName, []byte(strings.Repeat("x", 1500)+"\x00"))},
		{"GNU zero-filled name payload", metadata(tar.TypeGNULongName, append([]byte("orphan\x00"), make([]byte, 2048)...))},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			archive := append(append([]byte(nil), base...), fixture.data...)
			if _, err := unpackImport(bytes.NewReader(archive), t.TempDir()); err == nil {
				t.Fatal("orphan metadata accepted as a complete archive")
			}
		})
	}
	// Extended names are valid when followed by an entry and actual terminators.
	for _, format := range []tar.Format{tar.FormatPAX, tar.FormatGNU} {
		name := strings.Repeat("x", 130)
		archive := importArchive(t, &tar.Header{Name: name, Typeflag: tar.TypeReg, Size: 7, Format: format})
		if size, err := unpackImport(bytes.NewReader(archive), t.TempDir()); err != nil || size != 7 {
			t.Fatalf("valid extended name rejected: format=%v size=%d err=%v", format, size, err)
		}
	}
}
