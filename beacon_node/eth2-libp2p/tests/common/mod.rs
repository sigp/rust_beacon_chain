#![cfg(test)]
use eth2_libp2p::Enr;
use eth2_libp2p::EnrExt;
use eth2_libp2p::Multiaddr;
use eth2_libp2p::NetworkConfig;
use eth2_libp2p::Service as LibP2PService;
use slog::{debug, error, o, Drain};
use std::net::{TcpListener, UdpSocket};
use std::time::Duration;
use types::{EnrForkId, MinimalEthSpec};

type E = MinimalEthSpec;
use tempdir::TempDir;

pub fn build_log(level: slog::Level, enabled: bool) -> slog::Logger {
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    if enabled {
        slog::Logger::root(drain.filter_level(level).fuse(), o!())
    } else {
        slog::Logger::root(drain.filter(|_| false).fuse(), o!())
    }
}

// A bit of hack to find an unused port.
///
/// Does not guarantee that the given port is unused after the function exists, just that it was
/// unused before the function started (i.e., it does not reserve a port).
pub fn unused_port(transport: &str) -> Result<u16, String> {
    let local_addr = match transport {
        "tcp" => {
            let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| {
                format!("Failed to create TCP listener to find unused port: {:?}", e)
            })?;
            listener.local_addr().map_err(|e| {
                format!(
                    "Failed to read TCP listener local_addr to find unused port: {:?}",
                    e
                )
            })?
        }
        "udp" => {
            let socket = UdpSocket::bind("127.0.0.1:0")
                .map_err(|e| format!("Failed to create UDP socket to find unused port: {:?}", e))?;
            socket.local_addr().map_err(|e| {
                format!(
                    "Failed to read UDP socket local_addr to find unused port: {:?}",
                    e
                )
            })?
        }
        _ => return Err("Invalid transport to find unused port".into()),
    };
    Ok(local_addr.port())
}

pub fn build_config(
    port: u16,
    mut boot_nodes: Vec<Enr>,
    secret_key: Option<String>,
) -> NetworkConfig {
    let mut config = NetworkConfig::default();
    let path = TempDir::new(&format!("libp2p_test{}", port)).unwrap();

    config.libp2p_port = port; // tcp port
    config.discovery_port = port; // udp port
    config.enr_tcp_port = Some(port);
    config.enr_udp_port = Some(port);
    config.enr_address = Some("127.0.0.1".parse().unwrap());
    config.boot_nodes.append(&mut boot_nodes);
    config.secret_key_hex = secret_key;
    config.network_dir = path.into_path();
    // Reduce gossipsub heartbeat parameters
    config.gs_config.heartbeat_initial_delay = Duration::from_millis(500);
    config.gs_config.heartbeat_interval = Duration::from_millis(500);
    config
}

pub fn build_libp2p_instance(
    boot_nodes: Vec<Enr>,
    secret_key: Option<String>,
    log: slog::Logger,
) -> LibP2PService<E> {
    let port = unused_port("tcp").unwrap();
    let config = build_config(port, boot_nodes, secret_key);
    // launch libp2p service
    LibP2PService::new(&config, EnrForkId::default(), log.clone())
        .expect("should build libp2p instance")
        .1
}

#[allow(dead_code)]
pub fn get_enr(node: &LibP2PService<E>) -> Enr {
    let enr = node.swarm.discovery().local_enr().clone();
    enr
}

// Returns `n` libp2p peers in fully connected topology.
#[allow(dead_code)]
pub fn build_full_mesh(log: slog::Logger, n: usize) -> Vec<LibP2PService<E>> {
    let mut nodes: Vec<LibP2PService<E>> = (0..n)
        .map(|_| build_libp2p_instance(vec![], None, log.clone()))
        .collect();
    let multiaddrs: Vec<Multiaddr> = nodes
        .iter()
        .map(|x| get_enr(&x).multiaddr()[1].clone())
        .collect();

    for (i, node) in nodes.iter_mut().enumerate().take(n) {
        for (j, multiaddr) in multiaddrs.iter().enumerate().skip(i) {
            if i != j {
                match libp2p::Swarm::dial_addr(&mut node.swarm, multiaddr.clone()) {
                    Ok(()) => debug!(log, "Connected"),
                    Err(_) => error!(log, "Failed to connect"),
                };
            }
        }
    }
    nodes
}

// Constructs a pair of nodes with separate loggers. The sender dials the receiver.
// This returns a (sender, receiver) pair.
#[allow(dead_code)]
pub fn build_node_pair(log: &slog::Logger) -> (LibP2PService<E>, LibP2PService<E>) {
    let sender_log = log.new(o!("who" => "sender"));
    let receiver_log = log.new(o!("who" => "receiver"));

    let mut sender = build_libp2p_instance(vec![], None, sender_log);
    let receiver = build_libp2p_instance(vec![], None, receiver_log);

    let receiver_multiaddr = receiver.swarm.discovery().local_enr().clone().multiaddr()[1].clone();
    match libp2p::Swarm::dial_addr(&mut sender.swarm, receiver_multiaddr.clone()) {
        Ok(()) => {
            debug!(log, "Sender dialed receiver"; "address" => format!("{:?}", receiver_multiaddr))
        }
        Err(_) => error!(log, "Dialing failed"),
    };
    (sender, receiver)
}

// Returns `n` peers in a linear topology
#[allow(dead_code)]
pub fn build_linear(log: slog::Logger, n: usize) -> Vec<LibP2PService<E>> {
    let mut nodes: Vec<LibP2PService<E>> = (0..n)
        .map(|_| build_libp2p_instance(vec![], None, log.clone()))
        .collect();
    let multiaddrs: Vec<Multiaddr> = nodes
        .iter()
        .map(|x| get_enr(&x).multiaddr()[1].clone())
        .collect();
    for i in 0..n - 1 {
        match libp2p::Swarm::dial_addr(&mut nodes[i].swarm, multiaddrs[i + 1].clone()) {
            Ok(()) => debug!(log, "Connected"),
            Err(_) => error!(log, "Failed to connect"),
        };
    }
    nodes
}
