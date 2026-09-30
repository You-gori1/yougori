package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"syscall"

	"golang.org/x/sys/unix"
)

type storageCopier struct {
	ctx   context.Context
	links map[[2]uint64]string
	buf   []byte
}

// The daemon has not started during migration. Copy only these owned trees;
// symlinks, sparse extents, whiteouts, hardlinks and file capabilities survive.
// Every copied data extent is read back and verified before originals move.
func copyContainerStorage(ctx context.Context, source, destination string) error {
	info, err := os.Lstat(source)
	if err != nil {
		return err
	}
	if !info.IsDir() {
		return errors.New("container storage source is not a directory")
	}
	copier := storageCopier{ctx: ctx, links: make(map[[2]uint64]string), buf: make([]byte, 512*1024)}
	return copier.copy(source, destination, info.Sys().(*syscall.Stat_t).Dev)
}

func (c *storageCopier) copy(source, destination string, device uint64) error {
	if err := c.ctx.Err(); err != nil {
		return err
	}
	info, err := os.Lstat(source)
	if err != nil {
		return err
	}
	stat := info.Sys().(*syscall.Stat_t)
	if stat.Dev != device {
		return errors.New("container storage still contains a mounted share; disconnect it before updating")
	}
	key := [2]uint64{stat.Dev, stat.Ino}
	switch {
	case info.IsDir():
		if err := os.Mkdir(destination, 0700); err != nil {
			return err
		}
		dir, err := os.Open(source)
		if err != nil {
			return err
		}
		defer dir.Close()
		for {
			entries, readErr := dir.ReadDir(256)
			if readErr != nil && readErr != io.EOF {
				return readErr
			}
			for _, entry := range entries {
				if err := c.copy(filepath.Join(source, entry.Name()), filepath.Join(destination, entry.Name()), device); err != nil {
					return err
				}
			}
			if readErr == io.EOF {
				break
			}
		}
	case info.Mode().IsRegular():
		if previous, ok := c.links[key]; ok {
			if err := os.Link(previous, destination); err != nil {
				return err
			}
		} else {
			if err := c.copyFile(source, destination, info.Size()); err != nil {
				return err
			}
			if stat.Nlink > 1 {
				c.links[key] = destination
			}
		}
	case info.Mode()&os.ModeSymlink != 0:
		target, err := os.Readlink(source)
		if err != nil {
			return err
		}
		if err := os.Symlink(target, destination); err != nil {
			return err
		}
	default:
		if err := unix.Mknod(destination, stat.Mode, int(stat.Rdev)); err != nil {
			return err
		}
	}
	if err := os.Lchown(destination, int(stat.Uid), int(stat.Gid)); err != nil {
		return err
	}
	if info.Mode()&os.ModeSymlink == 0 {
		if err := unix.Chmod(destination, stat.Mode&07777); err != nil {
			return err
		}
	}
	if err := copyStorageAttributes(source, destination); err != nil {
		return err
	}
	times := []unix.Timespec{{Sec: stat.Atim.Sec, Nsec: stat.Atim.Nsec}, {Sec: stat.Mtim.Sec, Nsec: stat.Mtim.Nsec}}
	if err := unix.UtimesNanoAt(unix.AT_FDCWD, destination, times, unix.AT_SYMLINK_NOFOLLOW); err != nil {
		return err
	}
	if info.IsDir() || info.Mode().IsRegular() {
		file, err := os.Open(destination)
		if err != nil {
			return err
		}
		err = file.Sync()
		file.Close()
		return err
	}
	return nil
}

func (c *storageCopier) copyFile(source, destination string, size int64) error {
	input, err := os.Open(source)
	if err != nil {
		return err
	}
	defer input.Close()
	output, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_RDWR, 0600)
	if err != nil {
		return err
	}
	defer output.Close()
	if err := output.Truncate(size); err != nil {
		return err
	}
	for offset := int64(0); offset < size; {
		if err := c.ctx.Err(); err != nil {
			return err
		}
		start, err := unix.Seek(int(input.Fd()), offset, unix.SEEK_DATA)
		if errors.Is(err, unix.ENXIO) {
			break // Remaining bytes are a hole in the freshly truncated copy.
		}
		end := size
		if errors.Is(err, unix.EINVAL) {
			start, err = offset, nil // Filesystem without sparse-extent queries.
		} else if err == nil {
			end, err = unix.Seek(int(input.Fd()), start, unix.SEEK_HOLE)
		}
		if err != nil {
			return err
		}
		end = min(end, size)
		if start < offset || end <= start {
			return errors.New("invalid storage extent")
		}
		if _, err = input.Seek(start, io.SeekStart); err != nil {
			return err
		}
		if _, err = output.Seek(start, io.SeekStart); err != nil {
			return err
		}
		expected := sha256.New()
		length := end - start
		n, err := io.CopyBuffer(io.MultiWriter(output, expected), io.LimitReader(input, length), c.buf)
		if err != nil || n != length {
			return fmt.Errorf("copy storage extent: wrote %d of %d bytes: %v", n, length, err)
		}
		if err := output.Sync(); err != nil {
			return err
		}
		if _, err := output.Seek(start, io.SeekStart); err != nil {
			return err
		}
		actual := sha256.New()
		n, err = io.CopyBuffer(actual, io.LimitReader(output, length), c.buf)
		if err != nil || n != length || !bytes.Equal(expected.Sum(nil), actual.Sum(nil)) {
			return errors.New("storage copy verification failed; original files were kept")
		}
		offset = end
	}
	return output.Sync()
}

func copyStorageAttributes(source, destination string) error {
	size, err := unix.Llistxattr(source, nil)
	if errors.Is(err, unix.ENOTSUP) {
		return nil
	}
	if err != nil {
		return err
	}
	names := make([]byte, size)
	size, err = unix.Llistxattr(source, names)
	if err != nil {
		return err
	}
	for _, name := range strings.Split(strings.TrimRight(string(names[:size]), "\x00"), "\x00") {
		if name == "" {
			continue
		}
		length, err := unix.Lgetxattr(source, name, nil)
		if err != nil {
			return err
		}
		value := make([]byte, length)
		length, err = unix.Lgetxattr(source, name, value)
		if err != nil {
			return err
		}
		if err := unix.Lsetxattr(destination, name, value[:length], 0); err != nil {
			return fmt.Errorf("preserve storage attribute %s: %w", name, err)
		}
	}
	return nil
}
