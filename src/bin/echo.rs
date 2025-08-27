use std::{collections::{BTreeMap}, net::SocketAddr, ops::{BitOr, Shl}, sync::Arc};
use dashmap::DashMap;
use tokio::{net::UdpSocket, runtime::Builder, sync::Mutex, time::{Duration, interval}};

const BUFFER_SIZE: usize = 172;
const TIMEOUT_MS: u64 = 500;
const INTERVAL_MS: u64 = 20;

type RTPBuffer = [u8; BUFFER_SIZE];

struct RTPPacket {
    seqno: u16,
    data: RTPBuffer
}

type SSRC = u32;
type SsrcToBuffers = Arc<DashMap<SSRC, Arc<Mutex<BTreeMap<u16, RTPPacket>>>>>;


fn main() -> std::io::Result<()> {
    let runtime = Builder::new_multi_thread().enable_all().build().unwrap();

    runtime.block_on(async {
        let socket = Arc::from(UdpSocket::bind("0.0.0.0:5004").await?);

        let ssrc_to_buffer: SsrcToBuffers = Arc::from(DashMap::new());

        loop {
            let mut buf: RTPBuffer = [0; BUFFER_SIZE];
            let (_len, addr) = socket.recv_from(&mut buf).await?;

            let ssrc_to_buffers = Arc::clone(&ssrc_to_buffer);
            let socket_clone = socket.clone();

            tokio::spawn(async move {
                handle_packet(buf, addr, ssrc_to_buffers, socket_clone).await;
            });
        }
    })
}

async fn handle_packet(buf: RTPBuffer, from: SocketAddr, ssrc_to_buffers: SsrcToBuffers, socket: Arc<UdpSocket>) {
        let version = &buf[0] >> 6;
        println!("Version: {version}");
         
        if version != 2 {
            println!("{}", std::str::from_utf8(&buf).unwrap())
        }
        
        
        let ssrc = read_ubytes::<SSRC>(&buf, 8, 4);
        let seqno = read_ubytes::<u16>(&buf, 2, 2);
        let packet = RTPPacket {
            seqno,
            data: buf 
        };
        let mut need_interval = false;
        match ssrc_to_buffers.get(&ssrc) {
            Some(_) => {
                let buffer_arc = ssrc_to_buffers.get(&ssrc).unwrap();
                let mut buffer = buffer_arc.lock().await;
                buffer.insert(packet.seqno, packet);
            },
            None => {
                println!("spawned! {}", ssrc);
                need_interval = true;
                ssrc_to_buffers.insert(ssrc, Arc::new(Mutex::new(BTreeMap::from([(packet.seqno, packet)]))));
            }
        }

        if need_interval {
            tokio::spawn(async move {
                let mut interval = interval(Duration::from_millis(INTERVAL_MS));
                let mut passes_ms = 0;
                loop {
                    interval.tick().await;
                    let buffer_arc = ssrc_to_buffers.get(&ssrc).unwrap();
                    let mut buffer = buffer_arc.lock().await;
                    if let Some((_, packet)) = buffer.pop_first() {
                        //SocketAddr::new(from.ip(), 5006)
                        if let Err(error) = socket.send_to(&packet.data, SocketAddr::new(from.ip(), from.port() + 1)).await {
                            println!("Failed to send packet! ({})", error.kind());
                        }
                        passes_ms = 0;
                        continue;
                    } else {
                            passes_ms += INTERVAL_MS;
                    }
                    if passes_ms > TIMEOUT_MS {
                            ssrc_to_buffers.remove(&ssrc);
                            break;
                    }
                }
            });
        }
}

fn read_ubytes<T>(buffer: &[u8], start: usize, bytes: usize) -> T
where T: Default + Copy + Shl<usize, Output = T> + BitOr<Output = T> + From<u8> {
    let mut offset: i32 = bytes as i32 * 8 - 8;
    let mut acc: T = T::from(0);

    for i in start..start + bytes {
        acc = acc | (T::from(buffer[i])) << offset as usize;
        offset -= 8;
    }

    acc
}