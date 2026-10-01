package main

// nerdctl export omits image-declared volume data. Merge owned volume files
// into their guest paths while streaming, without following guest symlinks or
// including My PC shares, other nodes' connection mounts, proc/sys/dev mounts.
import (
	"archive/tar"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"
	"syscall"

	"golang.org/x/sys/unix"
)

type snapshotVolume struct{ Source, Destination string }

func privateSnapshotVolumes(mounts []snapshotVolume) ([]snapshotVolume, error) {
	var volumes []snapshotVolume
	seen := make(map[string]bool)
	for _, mount := range mounts {
		if !isPrivateContainerVolume(mount.Source) {
			continue
		}
		resolved, err := filepath.EvalSymlinks(mount.Source)
		if err != nil || resolved != mount.Source {
			return nil, errors.New("private volume was redirected or is unavailable")
		}
		if !strings.HasPrefix(mount.Destination, "/") || path.Clean(mount.Destination) != mount.Destination || mount.Destination == "/" || strings.ContainsAny(mount.Destination, "\\\x00") {
			return nil, errors.New("invalid guest volume path")
		}
		if seen[mount.Destination] {
			return nil, errors.New("duplicate guest volume destination")
		}
		seen[mount.Destination] = true
		volumes = append(volumes, mount)
	}
	sort.Slice(volumes, func(a, b int) bool { return volumes[a].Destination < volumes[b].Destination })
	return volumes, nil
}

func underVolume(name string, volumes []snapshotVolume) bool {
	name = strings.TrimPrefix(path.Clean(name), "/")
	for _, volume := range volumes {
		destination := strings.TrimPrefix(volume.Destination, "/")
		if name == destination || strings.HasPrefix(name, destination+"/") {
			return true
		}
	}
	return false
}

func mergeSnapshotVolumes(output io.Writer, rootfs io.Reader, volumes []snapshotVolume) error {
	if len(volumes) == 0 {
		_, err := io.Copy(output, rootfs)
		return err
	}
	archive := tar.NewWriter(output)
	input := tar.NewReader(rootfs)
	for {
		header, err := input.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return err
		}
		if underVolume(header.Name, volumes) {
			continue
		}
		if err = archive.WriteHeader(header); err != nil {
			return err
		}
		if _, err = io.Copy(archive, input); err != nil {
			return err
		}
	}
	// Consume export padding so the producer cannot block waiting for Wait().
	if _, err := io.Copy(io.Discard, rootfs); err != nil {
		return err
	}
	for index, volume := range volumes {
		root, err := os.OpenRoot(volume.Source)
		if err != nil {
			return err
		}
		err = appendSnapshotVolume(archive, root, strings.TrimPrefix(volume.Destination, "/"), volumes[index+1:])
		_ = root.Close()
		if err != nil {
			return err
		}
	}
	return archive.Close()
}

func appendSnapshotVolume(archive *tar.Writer, root *os.Root, destination string, nested []snapshotVolume) error {
	hardlinks := make(map[[2]uint64]string)
	return fs.WalkDir(root.FS(), ".", func(relative string, entry fs.DirEntry, walkError error) error {
		if walkError != nil {
			return walkError
		}
		name := path.Join(destination, relative)
		if underVolume(name, nested) {
			if entry.IsDir() {
				return fs.SkipDir
			}
			return nil
		}
		info, err := root.Lstat(relative)
		if err != nil {
			return err
		}
		if info.Mode()&os.ModeSocket != 0 {
			return nil
		} // sockets are live process endpoints, not portable files
		link := ""
		if info.Mode()&os.ModeSymlink != 0 {
			link, err = root.Readlink(relative)
			if err != nil {
				return err
			}
		}
		header, err := tar.FileInfoHeader(info, link)
		if err != nil {
			return err
		}
		header.Name = name
		if stat, ok := info.Sys().(*syscall.Stat_t); ok && info.Mode().IsRegular() && stat.Nlink > 1 {
			key := [2]uint64{uint64(stat.Dev), stat.Ino}
			if previous, ok := hardlinks[key]; ok {
				header.Typeflag = tar.TypeLink
				header.Linkname = previous
				header.Size = 0
			} else {
				hardlinks[key] = name
			}
		}
		var file *os.File
		if info.Mode().IsRegular() || info.IsDir() {
			file, err = root.Open(relative)
			if err != nil {
				return err
			}
			defer file.Close()
			opened, err := file.Stat()
			if err != nil {
				return err
			}
			if !os.SameFile(info, opened) {
				return fmt.Errorf("volume file changed during export: %s", name)
			}
			attributes, err := snapshotAttributes(file)
			if err != nil {
				return err
			}
			header.PAXRecords = attributes
		}
		if err = archive.WriteHeader(header); err != nil {
			return err
		}
		if header.Typeflag == tar.TypeReg {
			_, err = io.CopyN(archive, file, info.Size())
			return err
		}
		return nil
	})
}

func snapshotAttributes(file *os.File) (map[string]string, error) {
	fd := int(file.Fd())
	size, err := unix.Flistxattr(fd, nil)
	if errors.Is(err, unix.ENOTSUP) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if size == 0 {
		return nil, nil
	}
	if size > 65536 {
		return nil, errors.New("volume extended attributes exceed the size limit")
	}
	names := make([]byte, size)
	size, err = unix.Flistxattr(fd, names)
	if err != nil {
		return nil, err
	}
	attributes := make(map[string]string)
	for _, name := range strings.Split(strings.TrimRight(string(names[:size]), "\x00"), "\x00") {
		size, err := unix.Fgetxattr(fd, name, nil)
		if err != nil {
			return nil, err
		}
		if size > 65536 {
			return nil, errors.New("volume extended attribute exceeds the size limit")
		}
		value := make([]byte, size)
		size, err = unix.Fgetxattr(fd, name, value)
		if err != nil {
			return nil, err
		}
		attributes["SCHILY.xattr."+name] = string(value[:size])
	}
	return attributes, nil
}
