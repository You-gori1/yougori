package main

// Host-initiated, authenticated multiplexed loopback relay. WSL's NAT does not
// let a guest connect to Windows 127.0.0.1. Never open the host's LAN interface.
import (
	"encoding/binary"
	"fmt"
	"io"
	"net"
	"net/http"
	"sync"
	"time"
)

const relayMaxPayload = 65536
const relayMaxStreams = 16

type relayWriter struct {
	sync.Mutex
	connection net.Conn
}

func (w *relayWriter) frame(kind byte, id uint32, payload []byte) error {
	if len(payload) > relayMaxPayload {
		return fmt.Errorf("relay payload too large")
	}
	w.Lock()
	defer w.Unlock()
	var header [9]byte
	header[0] = kind
	binary.BigEndian.PutUint32(header[1:5], id)
	binary.BigEndian.PutUint32(header[5:9], uint32(len(payload)))
	w.connection.SetWriteDeadline(time.Now().Add(30 * time.Second))
	if _, err := w.connection.Write(header[:]); err != nil {
		return err
	}
	_, err := w.connection.Write(payload)
	w.connection.SetWriteDeadline(time.Time{})
	return err
}

var hostRelaySlots = make(chan struct{}, 64)

func (s *server) hostRelay(w http.ResponseWriter, r *http.Request) {
	if !cudaMode() {
		writeError(w, 400, "host relays are only available in the CUDA runtime")
		return
	}
	var request struct {
		ID string `json:"id"`
	}
	if !decodeRequest(w, r, &request) || !requireID(w, request.ID) {
		return
	}
	select {
	case hostRelaySlots <- struct{}{}:
		defer func() { <-hostRelaySlots }()
	default:
		writeError(w, 429, "too many host relays")
		return
	}
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		writeError(w, 500, err.Error())
		return
	}
	defer listener.Close()
	hijacker, ok := w.(http.Hijacker)
	if !ok {
		writeError(w, 500, "streaming is unavailable")
		return
	}
	connection, buffer, err := hijacker.Hijack()
	if err != nil {
		return
	}
	defer connection.Close()
	port := listener.Addr().(*net.TCPAddr).Port
	fmt.Fprintf(buffer, "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: yougori-relay\r\nYougori-Port: %d\r\n\r\n", port)
	if buffer.Flush() != nil {
		return
	}
	serveHostRelay(connection, listener)
}

func serveHostRelay(connection net.Conn, listener net.Listener) {
	writer := &relayWriter{connection: connection}
	var mu sync.Mutex
	streams := map[uint32]net.Conn{}
	closed := false
	defer func() {
		listener.Close()
		mu.Lock()
		closed = true
		for _, stream := range streams {
			stream.Close()
		}
		mu.Unlock()
	}()
	go func() {
		var next uint32
		for {
			stream, err := listener.Accept()
			if err != nil {
				return
			}
			mu.Lock()
			if closed || len(streams) >= relayMaxStreams || next == ^uint32(0) {
				mu.Unlock()
				stream.Close()
				continue
			}
			next++
			id := next
			streams[id] = stream
			mu.Unlock()
			if writer.frame(1, id, nil) != nil {
				connection.Close()
				return
			}
			go func() {
				defer func() { mu.Lock(); delete(streams, id); mu.Unlock(); stream.Close(); writer.frame(3, id, nil) }()
				bytes := make([]byte, relayMaxPayload)
				for {
					n, err := stream.Read(bytes)
					if n > 0 && writer.frame(2, id, bytes[:n]) != nil {
						connection.Close()
						return
					}
					if err != nil {
						return
					}
				}
			}()
		}
	}()
	for {
		var header [9]byte
		if _, err := io.ReadFull(connection, header[:]); err != nil {
			return
		}
		id, size := binary.BigEndian.Uint32(header[1:5]), binary.BigEndian.Uint32(header[5:9])
		if size > relayMaxPayload || id == 0 || (header[0] != 2 && header[0] != 3) || (header[0] == 3 && size != 0) {
			return
		}
		payload := make([]byte, size)
		if _, err := io.ReadFull(connection, payload); err != nil {
			return
		}
		mu.Lock()
		stream := streams[id]
		mu.Unlock()
		if stream == nil {
			continue
		}
		if header[0] == 3 {
			stream.Close()
			continue
		}
		stream.SetWriteDeadline(time.Now().Add(15 * time.Second))
		if _, err := stream.Write(payload); err != nil {
			stream.Close()
		}
		stream.SetWriteDeadline(time.Time{})
	}
}
