package main

import (
	"bytes"
	"compress/gzip"
	"encoding/binary"
	"errors"
	"io"
	"reflect"
	"testing"
)

func TestSnapshotStreamRejectsFailedAndCancelledExports(t *testing.T) {
	for _, fails := range []bool{false, true} {
		var output bytes.Buffer
		data := bytes.Repeat([]byte("database pages"), 4000)
		metadata := []byte(`{"architecture":"amd64","os":"linux","config":{"WorkingDir":"/project"}}`)
		err := writeSnapshotStream(&output, metadata, func(w io.Writer) error {
			if _, err := w.Write(data); err != nil {
				return err
			}
			if fails {
				return errors.New("export interrupted")
			}
			return nil
		})
		if (err != nil) != fails {
			t.Fatalf("unexpected export result: %v", err)
		}
		if string(output.Next(len(snapshotStreamMagic))) != snapshotStreamMagic {
			t.Fatal("invalid version")
		}
		var size uint32
		if err := binary.Read(&output, binary.BigEndian, &size); err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(output.Next(int(size)), metadata) {
			t.Fatal("metadata changed")
		}
		reader, err := gzip.NewReader(&output)
		if err != nil {
			t.Fatal(err)
		}
		got, err := io.ReadAll(reader)
		if fails {
			if err == nil {
				t.Fatal("failed export was accepted as a complete snapshot")
			}
		} else if err != nil || !bytes.Equal(got, data) {
			t.Fatalf("snapshot contents changed: %v", err)
		}
	}
}

func TestSnapshotPauseRemainsAliveButDoesNotRequestProcessStatistics(t *testing.T) {
	listed, err := parseNerdctlContainerList(`
{"Names":"snapshot","State":"paused","Status":"Paused"}
{"Names":"transition","State":"pausing","Status":"Pausing"}
{"Names":"docker-format","State":"paused","Status":"Up 2 seconds (Paused)"}
{"Names":"service","State":"running","Status":"Up 2 seconds"}
{"Names":"exited","State":"exited","Status":"Exited (0)"}`)
	if err != nil {
		t.Fatal(err)
	}
	entries, running := prepareStatsEntries([]string{"snapshot", "transition", "docker-format", "service", "exited"}, listed)
	for _, entry := range entries[:3] {
		if !entry.Running || !entry.Paused {
			t.Fatalf("paused task falsely exited: %+v", entry)
		}
	}
	if entries[3].Paused || entries[4].Running || !reflect.DeepEqual(running, []string{"service"}) {
		t.Fatalf("invalid task sampling: %+v / %v", entries, running)
	}
}
