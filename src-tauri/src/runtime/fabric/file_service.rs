//! A small HTTP endpoint on the existing private NIC, not a host/LAN bridge.
//! One isolated TCP stack per guest binds requests to the authenticated socket
//! identity. No additional guest adapter, credentials, gateway or guest agent.
use super::*;
use crate::runtime::connection_files::SharedFiles;
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    socket::tcp,
    time::Instant as Tick,
    wire::{EthernetAddress, IpAddress, IpCidr},
};
use std::collections::VecDeque;
pub const IP: [u8; 4] = [10, 192, 0, 1];
pub const MAC: [u8; 6] = [0x52, 0x54, 0x4f, 192, 0, 1];
pub(crate) struct Wire {
    pub incoming: VecDeque<Vec<u8>>,
    pub outgoing: mpsc::Sender<Vec<u8>>,
}
pub(crate) struct Rx(Vec<u8>);
pub(crate) struct Tx(mpsc::Sender<Vec<u8>>);
impl RxToken for Rx {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}
impl TxToken for Tx {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut bytes = vec![0; len];
        let result = f(&mut bytes);
        let _ = self.0.try_send(bytes);
        result
    }
}
impl Device for Wire {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx;
    fn receive(&mut self, _: Tick) -> Option<(Rx, Tx)> {
        self.incoming
            .pop_front()
            .map(|p| (Rx(p), Tx(self.outgoing.clone())))
    }
    fn transmit(&mut self, _: Tick) -> Option<Tx> {
        Some(Tx(self.outgoing.clone()))
    }
    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ethernet;
        c.max_transmission_unit = 1514;
        c
    }
}
struct Slot {
    handle: smoltcp::iface::SocketHandle,
    request: Vec<u8>,
    pending: Option<tokio::task::JoinHandle<Vec<u8>>>,
    response: Vec<u8>,
    sent: usize,
    closing: bool,
    touched: Instant,
}
impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(task) = &self.pending {
            task.abort()
        }
    }
}
fn request_size(bytes: &[u8]) -> Result<Option<usize>, ()> {
    let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if bytes.len() > 16384 {
            Err(())
        } else {
            Ok(None)
        };
    };
    if end > 16384 {
        return Err(());
    }
    let header = std::str::from_utf8(&bytes[..end]).map_err(|_| ())?;
    let mut length = None;
    for line in header.lines().skip(1) {
        let (name, value) = line.split_once(':').ok_or(())?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(());
            }
            length = Some(value.trim().parse::<usize>().map_err(|_| ())?);
        }
    }
    let size = length.unwrap_or(0);
    if size > 1024 * 1024 {
        return Err(());
    }
    Ok(Some(end + 4 + size))
}
pub fn start(
    id: String,
    files: SharedFiles,
    outgoing: mpsc::Sender<Vec<u8>>,
) -> mpsc::Sender<Vec<u8>> {
    let (tx, mut rx) = mpsc::channel(128);
    tokio::spawn(async move {
        let start = Instant::now();
        let mut wire = Wire {
            incoming: VecDeque::new(),
            outgoing,
        };
        let mut config = Config::new(EthernetAddress(MAC).into());
        config.random_seed =
            u64::from_le_bytes(*uuid::Uuid::new_v4().as_bytes().first_chunk::<8>().unwrap());
        let mut interface = Interface::new(config, &mut wire, Tick::from_millis(0));
        interface.update_ip_addrs(|ips| {
            ips.push(IpCidr::new(IpAddress::v4(10, 192, 0, 1), 11))
                .unwrap();
        });
        let mut sockets = SocketSet::new(vec![]);
        let mut slots = Vec::new();
        for _ in 0..8 {
            let mut socket = tcp::Socket::new(
                tcp::SocketBuffer::new(vec![0; 32768]),
                tcp::SocketBuffer::new(vec![0; 32768]),
            );
            socket.set_timeout(Some(smoltcp::time::Duration::from_secs(30)));
            socket.listen(7444).unwrap();
            slots.push(Slot {
                handle: sockets.add(socket),
                request: vec![],
                pending: None,
                response: vec![],
                sent: 0,
                closing: false,
                touched: Instant::now(),
            });
        }
        let wake = Arc::new(tokio::sync::Notify::new());
        loop {
            let tick = Tick::from_millis(start.elapsed().as_millis() as i64);
            interface.poll(tick, &mut wire, &mut sockets);
            for slot in &mut slots {
                let socket = sockets.get_mut::<tcp::Socket>(slot.handle);
                if !socket.is_open() {
                    if let Some(task) = slot.pending.take() {
                        task.abort()
                    }
                    slot.request.clear();
                    slot.response.clear();
                    slot.sent = 0;
                    slot.closing = false;
                    slot.touched = Instant::now();
                    socket.listen(7444).unwrap();
                }
                if slot.pending.is_none()
                    && slot.response.is_empty()
                    && !slot.closing
                    && socket.can_recv()
                {
                    let _ = socket.recv(|data| {
                        let count = data
                            .len()
                            .min(1024 * 1024 + 16388 - slot.request.len().min(1024 * 1024 + 16388));
                        slot.request.extend_from_slice(&data[..count]);
                        (count, ())
                    });
                    slot.touched = Instant::now();
                    match request_size(&slot.request) {
                        Ok(Some(size)) if slot.request.len() >= size => {
                            slot.request.truncate(size);
                            let request = std::mem::take(&mut slot.request);
                            let files = files.clone();
                            let id = id.clone();
                            let wake = wake.clone();
                            slot.pending = Some(tokio::spawn(async move {
                                let result = files.http(&id, &request).await;
                                wake.notify_one();
                                result
                            }));
                        }
                        Err(()) => socket.abort(),
                        _ => {}
                    }
                }
                if slot.pending.as_ref().is_some_and(|p| p.is_finished()) {
                    slot.response = slot.pending.take().unwrap().await.unwrap_or_default();
                    if slot.response.is_empty() {
                        socket.abort()
                    }
                }
                if socket.can_send() && slot.sent < slot.response.len() {
                    if let Ok(n) = socket.send_slice(&slot.response[slot.sent..]) {
                        slot.sent += n;
                        slot.touched = Instant::now();
                    }
                }
                if !slot.response.is_empty() && slot.sent == slot.response.len() && !slot.closing {
                    socket.close();
                    slot.closing = true;
                }
                if socket.is_active() && slot.touched.elapsed() > Duration::from_secs(30) {
                    socket.abort();
                }
            }
            interface.poll(tick, &mut wire, &mut sockets);
            let delay = interface
                .poll_delay(tick, &sockets)
                .map(|d| Duration::from_millis(d.total_millis()))
                .unwrap_or(Duration::from_secs(1))
                .clamp(Duration::from_millis(1), Duration::from_secs(1));
            tokio::select! {
                packet=rx.recv()=>{let Some(packet)=packet else{break};wire.incoming.push_back(packet);while wire.incoming.len()<128 {match rx.try_recv(){Ok(p)=>wire.incoming.push_back(p),Err(_)=>break}}},
                _=wake.notified()=>{},
                _=tokio::time::sleep(delay)=>{},
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn http_framing_is_bounded_and_unambiguous() {
        assert_eq!(
            request_size(b"GET / HTTP/1.1\r\nHost: test\r\n\r\n"),
            Ok(Some(30))
        );
        assert!(
            request_size(b"POST / HTTP/1.1\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n")
                .is_err()
        );
        assert!(request_size(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
        assert!(request_size(b"POST / HTTP/1.1\r\nContent-Length: 999999999\r\n\r\n").is_err());
        assert!(request_size(&vec![b'x'; 16385]).is_err());
    }
}
