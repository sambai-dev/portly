use std::collections::HashSet;
use std::io;

use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};

use crate::model::{PortEntry, Protocol, Source};

/// Snapshot of every listening TCP socket and bound UDP port.
/// One row per (proto, port): IPv4+IPv6 listeners collapse into a single line.
pub fn scan_sockets() -> io::Result<Vec<PortEntry>> {
    let sockets = get_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP | ProtocolFlags::UDP,
    )
    .map_err(|e| io::Error::other(e.to_string()))?;

    let mut out = Vec::new();
    let mut seen: HashSet<(u8, u16)> = HashSet::new();

    for si in sockets {
        match si.protocol_socket_info {
            ProtocolSocketInfo::Tcp(t) => {
                if t.state != TcpState::Listen {
                    continue;
                }
                if seen.insert((0, t.local_port)) {
                    out.push(PortEntry {
                        port: t.local_port,
                        proto: Protocol::Tcp,
                        pid: si.associated_pids.first().copied(),
                        process: None,
                        cmdline: None,
                        cpu: None,
                        mem_bytes: None,
                        source: Source::Kernel,
                    });
                }
            }
            ProtocolSocketInfo::Udp(u) => {
                if seen.insert((1, u.local_port)) {
                    out.push(PortEntry {
                        port: u.local_port,
                        proto: Protocol::Udp,
                        pid: si.associated_pids.first().copied(),
                        process: None,
                        cmdline: None,
                        cpu: None,
                        mem_bytes: None,
                        source: Source::Kernel,
                    });
                }
            }
        }
    }
    Ok(out)
}
