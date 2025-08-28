use std::{collections::BTreeMap, net::SocketAddr, sync::Arc, vec};
use dashmap::DashMap;
use tokio::{net::UdpSocket, runtime::Builder, sync::Mutex, time::{interval, Duration}};
use std::time::{Instant};

const BUFFER_SIZE: usize = 172;
const TIMEOUT_MS: u64 = 500;
const INTERVAL_MS: u64 = 20;

type RTPBuffer = [u8; BUFFER_SIZE];

struct RTPPacket {
    seqno: u16,
    timestamp: Instant,
    data: RTPBuffer
}

struct Statistics {
    elapseds: Vec<u128>,
    success_packets: u16,
    failed_packets: u16
}

type SSRC = u32;
type SsrcToBuffers = Arc<DashMap<SSRC, Arc<Mutex<BTreeMap<u16, RTPPacket>>>>>;


fn main() -> std::io::Result<()> {
    let runtime = Builder::new_multi_thread().enable_all().build().unwrap();

    runtime.block_on(async {
        let socket = Arc::from(UdpSocket::bind("0.0.0.0:5004").await?);

        let ssrc_to_buffer: SsrcToBuffers = Arc::from(DashMap::new());
        let statistics: Arc<Mutex<Statistics>> = Arc::from(Mutex::from(Statistics::from(
            Statistics { elapseds: vec![], success_packets: 0, failed_packets: 0 }
        )));

        loop {
            let mut buf: RTPBuffer = [0; BUFFER_SIZE];
            let (_len, addr) = socket.recv_from(&mut buf).await?;

            let ssrc_to_buffers = Arc::clone(&ssrc_to_buffer);
            let socket_clone = socket.clone();
            let stats_clone = statistics.clone();

            tokio::spawn(async move {
                println!("spawning!");
                handle_packet(buf, addr, ssrc_to_buffers, socket_clone, stats_clone).await;
            });
        }
    })
}

async fn handle_packet(buf: RTPBuffer, from: SocketAddr, ssrc_to_buffers: SsrcToBuffers, socket: Arc<UdpSocket>, statistics: Arc<Mutex<Statistics>>) {
        let version = &buf[0] >> 6;
         
        if version != 2 {
            panic!();
        }
        
        let ssrc = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);
        let seqno = u16::from_be_bytes([buf[2], buf[3]]);
        let packet = RTPPacket {
            seqno,
            timestamp: Instant::now(),
            data: buf 
        };
        let mut need_interval = false;
        match ssrc_to_buffers.get(&ssrc) {
            Some(buffer_arc) => {
                let mut buffer = buffer_arc.lock().await;
                buffer.insert(packet.seqno, packet);
            },
            None => {
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
                        let mut failed = false;
                        if let Err(error) = socket.send_to(&packet.data, SocketAddr::new(from.ip(), from.port() + 1)).await {
                            println!("Failed to send packet! ({})", error.kind());
                            failed = true;
                        }
                        passes_ms = 0;
                        let mut stats = statistics.lock().await;
                        if failed {
                            stats.failed_packets += 1;
                        } else {
                            stats.success_packets += 1;
                        }
                        stats.elapseds.push(packet.timestamp.elapsed().as_millis());
                        continue;
                    } else {
                            passes_ms += INTERVAL_MS;
                    }

                    if passes_ms > TIMEOUT_MS {
                        let mut stats = statistics.lock().await;
                        println!("p50: {}; p95: {}; p99: {}", percentile_u128(&stats.elapseds, 0.5).unwrap(), percentile_u128(&stats.elapseds, 0.95).unwrap(), percentile_u128(&stats.elapseds, 0.99).unwrap());
                        println!("succed: {} / failed: {}", stats.success_packets, stats.failed_packets);
                        stats.failed_packets = 0;
                        stats.success_packets = 0;
                        stats.elapseds = vec![];

                        
                        break;
                    }
                }
            });
        }
}

fn percentile_u128(data: &[u128], p: f64) -> Option<f64> {
    if data.is_empty() {
        return None;
    }
    let p = p.clamp(0.0, 1.0);

    let mut v = data.to_vec();
    v.sort_unstable();

    let n = v.len();
    if n == 1 {
        return Some(v[0] as f64);
    }

    let h = (n as f64 - 1.0) * p;
    let i = h.floor() as usize;
    let frac = h - i as f64;

    if i + 1 < n {
        let x0 = v[i] as f64;
        let x1 = v[i + 1] as f64;
        Some(x0 + frac * (x1 - x0))
    } else {
        Some(v[i] as f64)
    }
}