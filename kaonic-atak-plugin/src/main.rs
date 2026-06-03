use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use cot::{parse_cot_payload, LocationState, PacketSource};
use interface::{
    load_interface_candidates, select_local_interface, InterfaceSelection, LocalInterface,
};
use kaonic_gateway::radio::{
    attach_selected_radio_interface, connect_radio_client, SharedRadioClient,
};
use kaonic_gateway::settings::Settings;
use multicast::{open_multicast_sockets, AtakChannel, ATAK_CHANNELS};
use rand::rngs::OsRng;
use reticulum::destination::link::LinkEvent;
use reticulum::destination::DestinationName;
use reticulum::hash::AddressHash;
use reticulum::identity::PrivateIdentity;
use reticulum::transport::{TimerConfig, Transport, TransportConfig};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod cot;
mod interface;
mod multicast;

const DEFAULT_DB_PATH: &str = "/kaonic-gateway.db";
const DEFAULT_CTRL_SERVER: &str = "192.168.10.1:9090";
const DEFAULT_SEED_KEY: &str = "atak_plugin_identity_seed";

#[derive(Parser)]
#[command(name = "kaonic-atak-plugin", version)]
struct Command {
    #[arg(short = 'a', long)]
    kaonic_ctrl_server: Option<SocketAddr>,

    #[arg(long, default_value_t = 0)]
    rns_module: usize,

    #[arg(long, default_value = DEFAULT_SEED_KEY)]
    seed_key: String,

    #[arg(long, value_name = "IFACE")]
    local_interface: Option<String>,

    #[arg(long, value_name = "IPv4")]
    local_address: Option<Ipv4Addr>,

    #[arg(long, default_value_t = false)]
    allow_unvalidated_payloads: bool,
}

#[derive(Default)]
struct BridgeMetrics {
    local_rx_packets: AtomicU64,
    remote_tx_packets: AtomicU64,
    invalid_local_packets: AtomicU64,
    invalid_remote_packets: AtomicU64,
    local_locations: AtomicU64,
    remote_locations: AtomicU64,
}

#[tokio::main]
async fn main() -> Result<(), process::ExitCode> {
    let cmd = Command::parse();

    env_logger::Builder::new()
        .parse_filters("warn,kaonic_atak_plugin=debug,kaonic_gateway=warn,reticulum=warn")
        .parse_default_env()
        .init();

    let selection = InterfaceSelection {
        interface_name: cmd.local_interface.clone(),
        local_addr: cmd.local_address,
    };
    let local_interface = select_local_interface(&load_interface_candidates(), &selection)
        .map_err(|err| {
            log::error!("local ATAK interface selection failed: {err}");
            process::ExitCode::FAILURE
        })?;
    log::info!(
        "using local ATAK interface {} ({})",
        local_interface.name,
        local_interface.addr
    );

    if cmd.allow_unvalidated_payloads {
        log::warn!("unvalidated ATAK payload forwarding is enabled by command-line override");
    }

    let db_path =
        std::env::var("KAONIC_GATEWAY_DB_PATH").unwrap_or_else(|_| DEFAULT_DB_PATH.to_string());
    let settings = Settings::open(&db_path).unwrap_or_else(|err| {
        log::error!("failed to open database {db_path}: {err}");
        process::exit(1);
    });
    let config = settings.load_config().unwrap_or_else(|err| {
        log::error!("failed to load config from database: {err}");
        process::exit(1);
    });

    if cmd.rns_module >= config.radio.module_configs.len() {
        log::error!("radio module {} not found", cmd.rns_module);
        return Err(process::ExitCode::FAILURE);
    }

    let seed = settings
        .load_or_create_named_seed(&cmd.seed_key)
        .unwrap_or_else(|err| {
            log::error!(
                "failed to load/create ATAK identity seed '{}': {err}",
                cmd.seed_key
            );
            process::exit(1);
        });
    let id = PrivateIdentity::new_from_name(&seed);

    let server_addr = cmd
        .kaonic_ctrl_server
        .unwrap_or_else(|| DEFAULT_CTRL_SERVER.parse().unwrap());
    let listen_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let radio_client = connect_radio_client(listen_addr, server_addr)
        .await
        .map_err(|err| {
            log::error!("kaonic-ctrl connect error: {err:?}");
            process::ExitCode::FAILURE
        })?;

    let mut transport_cfg = TransportConfig::new("kaonic-atak-plugin", &id, true);
    transport_cfg.set_retransmit(true);
    transport_cfg.set_timer_config(TimerConfig {
        in_link_stale: Duration::from_secs(30),
        in_link_close: Duration::from_secs(15),
        out_link_restart: Duration::from_secs(45),
        out_link_stale: Duration::from_secs(30),
        out_link_close: Duration::from_secs(15),
        out_link_repeat: Duration::from_secs(10),
        out_link_keep: Duration::from_secs(5),
        ..TimerConfig::default()
    });
    transport_cfg.set_restart_outlinks(true);
    let transport = Arc::new(Mutex::new(Transport::new(transport_cfg)));
    attach_selected_radio_interface(
        &transport,
        radio_client.clone(),
        &config.radio,
        cmd.rns_module,
        None,
        None,
    )
    .await
    .map_err(|err| {
        log::error!("radio interface attach error: {err}");
        process::ExitCode::FAILURE
    })?;

    let cancel = CancellationToken::new();
    spawn_keepalive(radio_client, cancel.clone());

    let location_state = Arc::new(LocationState::default());
    let mut tasks = Vec::new();
    for channel in ATAK_CHANNELS {
        let channel_tasks = start_bridge(
            transport.clone(),
            id.clone(),
            *channel,
            local_interface.clone(),
            location_state.clone(),
            cmd.allow_unvalidated_payloads,
            cancel.clone(),
        )
        .await
        .map_err(|err| {
            log::error!(
                "failed to start ATAK channel {}:{}: {err}",
                channel.group,
                channel.port
            );
            process::ExitCode::FAILURE
        })?;
        tasks.extend(channel_tasks);
    }

    shutdown_signal(cancel.clone()).await;
    for task in tasks {
        if let Err(err) = task.await {
            log::warn!("bridge task join error: {err}");
        }
    }

    Ok(())
}

async fn start_bridge(
    transport: Arc<Mutex<Transport>>,
    identity: PrivateIdentity,
    channel: AtakChannel,
    local_interface: LocalInterface,
    location_state: Arc<LocationState>,
    allow_unvalidated_payloads: bool,
    cancel: CancellationToken,
) -> Result<Vec<JoinHandle<()>>, Box<dyn std::error::Error + Send + Sync>> {
    let metrics = Arc::new(BridgeMetrics::default());
    let port_tag = channel.port.to_be_bytes();
    let dest_name = format!("atak.{}", channel.port);
    let peer_hashes = Arc::new(Mutex::new(HashSet::<AddressHash>::new()));

    let destination = transport
        .lock()
        .await
        .add_destination(identity, DestinationName::new("kaonic", &dest_name))
        .await;
    let dest_hash = destination.lock().await.desc.address_hash;

    if let Ok(pkt) = destination.lock().await.announce(OsRng, Some(&port_tag)) {
        transport.lock().await.send_packet(pkt).await;
    }

    let sockets = open_multicast_sockets(channel, &local_interface)?;
    log::info!(
        "atak-plugin:{}:{} joined via {} ({}) dest={}",
        channel.name,
        channel.port,
        local_interface.name,
        local_interface.addr,
        dest_hash
    );

    let udp_to_rns = {
        let transport = transport.clone();
        let metrics = metrics.clone();
        let cancel = cancel.clone();
        let udp_rx = sockets.receiver.clone();
        let location_state = location_state.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    result = udp_rx.recv_from(&mut buf) => {
                        match result {
                            Ok((len, src)) => {
                                let data = &buf[..len];
                                if !accept_payload(
                                    data,
                                    PacketSource::LocalUdp,
                                    channel,
                                    &location_state,
                                    allow_unvalidated_payloads,
                                    &metrics.invalid_local_packets,
                                    &metrics.local_locations,
                                ) {
                                    continue;
                                }
                                log::debug!("atak-plugin:{}:{} udp -> rns {}B from {}", channel.name, channel.port, len, src);
                                metrics.local_rx_packets.fetch_add(1, Ordering::Relaxed);
                                transport.lock().await.send_to_in_links(&dest_hash, data).await;
                            }
                            Err(err) => log::warn!("atak-plugin:{}:{} udp receive error: {err}", channel.name, channel.port),
                        }
                    }
                }
            }
        })
    };

    let rns_to_udp = {
        let transport = transport.clone();
        let metrics = metrics.clone();
        let cancel = cancel.clone();
        let sender = sockets.sender.clone();
        let target = sockets.target;
        let peer_hashes = peer_hashes.clone();
        let location_state = location_state.clone();
        tokio::spawn(async move {
            let mut events = transport.lock().await.out_link_events();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    result = events.recv() => {
                        match result {
                            Ok(ev) => {
                                let tracked_peer = peer_hashes.lock().await.contains(&ev.address_hash);
                                if !tracked_peer {
                                    continue;
                                }

                                match ev.event {
                                    LinkEvent::Activated => {
                                        log::info!("atak-plugin:{}:{} link activated {}", channel.name, channel.port, ev.address_hash);
                                    }
                                    LinkEvent::Closed => {
                                        peer_hashes.lock().await.remove(&ev.address_hash);
                                        log::info!("atak-plugin:{}:{} link closed {}", channel.name, channel.port, ev.address_hash);
                                    }
                                    LinkEvent::Data(payload) => {
                                        let data = payload.as_slice();
                                        if !accept_payload(
                                            data,
                                            PacketSource::RemoteReticulum,
                                            channel,
                                            &location_state,
                                            allow_unvalidated_payloads,
                                            &metrics.invalid_remote_packets,
                                            &metrics.remote_locations,
                                        ) {
                                            continue;
                                        }
                                        if let Err(err) = sender.send_to(data, target).await {
                                            log::warn!("atak-plugin:{}:{} udp send error: {err}", channel.name, channel.port);
                                            continue;
                                        }
                                        metrics.remote_tx_packets.fetch_add(1, Ordering::Relaxed);
                                    }
                                    LinkEvent::Proof(_) => {}
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                log::warn!("atak-plugin:{}:{} skipped {skipped} Reticulum link events", channel.name, channel.port);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        })
    };

    let auto_link = {
        let transport = transport.clone();
        let cancel = cancel.clone();
        let peer_hashes = peer_hashes.clone();
        tokio::spawn(async move {
            let mut announces = transport.lock().await.recv_announces().await;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    result = announces.recv() => {
                        match result {
                            Ok(ev) => {
                                if ev.app_data.as_slice() != port_tag {
                                    continue;
                                }
                                let peer = ev.destination.lock().await.desc.clone();
                                if peer.address_hash == dest_hash {
                                    continue;
                                }

                                peer_hashes.lock().await.insert(peer.address_hash);
                                log::info!("atak-plugin:{}:{} auto-link -> {}", channel.name, channel.port, peer.address_hash);
                                transport.lock().await.link(peer).await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                log::warn!("atak-plugin:{}:{} skipped {skipped} Reticulum announces", channel.name, channel.port);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        })
    };

    let reannounce = {
        let transport = transport.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tick.tick() => {
                        if let Ok(pkt) = destination.lock().await.announce(OsRng, Some(&port_tag)) {
                            transport.lock().await.send_packet(pkt).await;
                        }
                    }
                }
            }
        })
    };

    Ok(vec![udp_to_rns, rns_to_udp, auto_link, reannounce])
}

fn accept_payload(
    data: &[u8],
    source: PacketSource,
    channel: AtakChannel,
    location_state: &LocationState,
    allow_unvalidated_payloads: bool,
    invalid_counter: &AtomicU64,
    location_counter: &AtomicU64,
) -> bool {
    match parse_cot_payload(data) {
        Ok(event) => {
            let record = location_state.record(source, channel.port, &event);
            let known_locations = location_state.len();
            log::debug!(
                "atak-plugin:{}:{} CoT source={:?} uid={} type={} lat={} lon={} recorded_port={} updated_at={:?} known_locations={}",
                channel.name,
                channel.port,
                record.source,
                record.uid,
                record.event_type,
                record.point.lat,
                record.point.lon,
                record.channel_port,
                record.updated_at,
                known_locations
            );
            location_counter.fetch_add(1, Ordering::Relaxed);
            true
        }
        Err(err) if allow_unvalidated_payloads => {
            invalid_counter.fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "atak-plugin:{}:{} forwarding unvalidated payload by override: {err}",
                channel.name,
                channel.port
            );
            true
        }
        Err(err) => {
            invalid_counter.fetch_add(1, Ordering::Relaxed);
            log::warn!(
                "atak-plugin:{}:{} dropping invalid ATAK payload: {err}",
                channel.name,
                channel.port
            );
            false
        }
    }
}

fn spawn_keepalive(radio_client: SharedRadioClient, cancel: CancellationToken) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(err) = radio_client.lock().await.ping().await {
                        log::warn!("keepalive ping failed: {err:?}");
                    }
                }
                _ = cancel.cancelled() => break,
            }
        }
    });
}

async fn shutdown_signal(cancel: CancellationToken) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("failed to register SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => log::info!("received Ctrl-C"),
            _ = sigterm.recv() => log::info!("received SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        log::info!("received Ctrl-C");
    }
    log::info!("shutting down");
    cancel.cancel();
}
