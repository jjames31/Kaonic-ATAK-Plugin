use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clap::Parser;
use cot::{parse_cot_payload, LocationState, PacketSource};
use diagnostics::{
    DiagnosticAction, DiagnosticCommand, DiagnosticRecord, DiagnosticState, DEFAULT_ENABLE_SECONDS,
    MAX_COMMAND_BYTES, MAX_ENABLE_SECONDS,
};
use interface::{
    load_interface_candidates, select_local_interface, InterfaceSelection, LocalInterface,
};
use kaonic_gateway::radio::{
    attach_selected_radio_interface, connect_radio_client, SharedRadioClient,
};
use kaonic_gateway::settings::Settings;
use multicast::{open_multicast_sockets, AtakChannel, ATAK_CHANNELS};
use rand::{rngs::OsRng, RngCore};
use reticulum::destination::link::LinkEvent;
use reticulum::destination::DestinationName;
use reticulum::hash::AddressHash;
use reticulum::identity::PrivateIdentity;
use reticulum::transport::{TimerConfig, Transport, TransportConfig};
use tokio::net::UdpSocket;
#[cfg(unix)]
use tokio::net::UnixDatagram;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

mod cot;
mod diagnostics;
mod interface;
mod multicast;

const DEFAULT_DB_PATH: &str = "/kaonic-gateway.db";
const DEFAULT_CTRL_SERVER: &str = "192.168.10.1:9090";
const DEFAULT_SEED_KEY: &str = "atak_plugin_identity_seed";
const ATAK_INTERFACE_ENV: &str = "KAONIC_ATAK_INTERFACE";
const ATAK_INTERFACE_IP_ENV: &str = "KAONIC_ATAK_INTERFACE_IP";
const OPAQUE_FORWARDING_ENV: &str = "KAONIC_ATAK_ALLOW_OPAQUE_FORWARDING";
const DIAGNOSTICS_UNIX_SOCKET_ENV: &str = "KAONIC_ATAK_DIAGNOSTICS_UNIX_SOCKET";
const DISABLE_LOCAL_DIAGNOSTICS_CONTROL_ENV: &str = "KAONIC_ATAK_DISABLE_LOCAL_DIAGNOSTICS_CONTROL";
const REQUIRE_DIAGNOSTICS_CONTROL_ENV: &str = "KAONIC_ATAK_REQUIRE_DIAGNOSTICS_CONTROL";
const DIAGNOSTICS_CONTROL_LISTEN_ENV: &str = "KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN";
const INSECURE_DIAGNOSTICS_CONTROL_LISTEN_ENV: &str =
    "KAONIC_ATAK_ALLOW_INSECURE_DIAGNOSTICS_CONTROL_LISTEN";
const UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL_ENV: &str =
    "KAONIC_ATAK_ENABLE_UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL";
const DEFAULT_DIAGNOSTICS_UNIX_SOCKET: &str = "/run/kaonic-atak-plugin/diagnostics.sock";
const DIAGNOSTICS_DEST_NAME: &str = "atak.diag.control";
const DIAGNOSTICS_PORT_TAG: &[u8] = b"atak-diag-control-v1";
const MAX_LOCAL_RECENT_RECORDS: usize = 20;
const MAX_LOCAL_DIAGNOSTIC_REQUEST_BYTES: usize = 256;

#[derive(Parser)]
#[command(name = "kaonic-atak-plugin", version)]
struct Command {
    #[arg(short = 'a', long)]
    kaonic_ctrl_server: Option<SocketAddr>,

    #[arg(long, default_value_t = 0)]
    rns_module: usize,

    #[arg(long, default_value = DEFAULT_SEED_KEY)]
    seed_key: String,

    /// ATAK-facing network interface name. Overrides KAONIC_ATAK_INTERFACE.
    #[arg(long, value_name = "IFACE")]
    local_interface: Option<String>,

    /// ATAK-facing IPv4 address. Overrides KAONIC_ATAK_INTERFACE_IP.
    #[arg(long, value_name = "IPv4")]
    local_address: Option<Ipv4Addr>,

    /// Explicit compatibility mode: forward payloads that are not valid CoT XML.
    #[arg(long, default_value_t = false)]
    allow_unvalidated_payloads: bool,

    /// Unix datagram socket used by a local diagnostics plugin or CLI. Overrides KAONIC_ATAK_DIAGNOSTICS_UNIX_SOCKET.
    #[arg(long, value_name = "PATH")]
    diagnostics_unix_socket: Option<PathBuf>,

    /// Disable the local diagnostics control endpoint. ATAK forwarding is unaffected.
    #[arg(long, default_value_t = false)]
    disable_local_diagnostics_control: bool,

    /// Treat local diagnostics control startup failure as fatal. Off by default so ATAK forwarding survives diagnostics issues.
    #[arg(long, default_value_t = false)]
    require_diagnostics_control: bool,

    /// Compatibility UDP address for local diagnostics tests. Disabled unless explicitly set.
    /// Overrides KAONIC_ATAK_DIAGNOSTICS_CONTROL_LISTEN.
    #[arg(long, value_name = "IPv4:PORT")]
    diagnostics_control_listen: Option<SocketAddr>,

    /// Permit diagnostics local-control binding outside loopback. Insecure; use only for controlled tests.
    #[arg(long, default_value_t = false)]
    allow_insecure_diagnostics_control_listen: bool,

    /// Enable unauthenticated network-wide diagnostics control. Insecure; use only on trusted test meshes.
    #[arg(long, default_value_t = false)]
    enable_unauthenticated_diagnostics_mesh_control: bool,
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

#[derive(Debug, Clone)]
struct DiagnosticsControlConfig {
    unix_socket: Option<PathBuf>,
    udp_addr: Option<SocketAddr>,
    require_control: bool,
}

#[derive(Debug, Clone, Copy)]
enum LocalDiagnosticRequest {
    Enable(u64),
    Disable,
    Status,
    Recent(usize),
}

#[tokio::main]
async fn main() -> Result<(), process::ExitCode> {
    let cmd = Command::parse();

    env_logger::Builder::new()
        .parse_filters("warn,kaonic_atak_plugin=info,kaonic_gateway=warn,reticulum=warn")
        .parse_default_env()
        .init();

    let selection = InterfaceSelection {
        interface_name: cmd
            .local_interface
            .clone()
            .or_else(|| non_empty_env(ATAK_INTERFACE_ENV)),
        local_addr: configured_ipv4(cmd.local_address, ATAK_INTERFACE_IP_ENV)?,
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

    let allow_unvalidated_payloads =
        cmd.allow_unvalidated_payloads || env_flag_enabled(OPAQUE_FORWARDING_ENV);
    if allow_unvalidated_payloads {
        log::warn!(
            "unvalidated ATAK payload forwarding is enabled by explicit compatibility override"
        );
    } else {
        log::info!("safe forwarding mode enabled: invalid non-CoT payloads will be dropped");
    }

    let disable_local_diagnostics_control = cmd.disable_local_diagnostics_control
        || env_flag_enabled(DISABLE_LOCAL_DIAGNOSTICS_CONTROL_ENV);
    let diagnostics_unix_socket = if disable_local_diagnostics_control {
        None
    } else {
        configured_diagnostics_unix_socket(cmd.diagnostics_unix_socket.clone())
    };
    let allow_insecure_diagnostics_control_listen = cmd.allow_insecure_diagnostics_control_listen
        || env_flag_enabled(INSECURE_DIAGNOSTICS_CONTROL_LISTEN_ENV);
    let diagnostics_udp_addr = configured_diagnostics_udp_addr(
        cmd.diagnostics_control_listen,
        allow_insecure_diagnostics_control_listen,
    );
    if allow_insecure_diagnostics_control_listen {
        if let Some(diagnostics_control_listen) = diagnostics_udp_addr {
            if !diagnostics_control_listen.ip().is_loopback() {
                log::warn!("insecure diagnostics UDP control binding allowed by explicit override");
            }
        }
    }
    let require_diagnostics_control =
        cmd.require_diagnostics_control || env_flag_enabled(REQUIRE_DIAGNOSTICS_CONTROL_ENV);
    let diagnostics_control = DiagnosticsControlConfig {
        unix_socket: diagnostics_unix_socket,
        udp_addr: diagnostics_udp_addr,
        require_control: require_diagnostics_control,
    };
    if diagnostics_control.unix_socket.is_none() && diagnostics_control.udp_addr.is_none() {
        log::warn!(
            "local diagnostics control is unavailable; ATAK bridge forwarding can still operate"
        );
    }

    let enable_unauthenticated_diagnostics_mesh_control = cmd
        .enable_unauthenticated_diagnostics_mesh_control
        || env_flag_enabled(UNAUTHENTICATED_DIAGNOSTICS_MESH_CONTROL_ENV);
    if enable_unauthenticated_diagnostics_mesh_control {
        log::warn!(
            "unauthenticated network-wide diagnostics control is enabled for this trusted test mesh"
        );
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
    let diagnostic_state = Arc::new(DiagnosticState::default());
    let mut tasks = vec![spawn_diagnostics_expiry_cleanup(
        diagnostic_state.clone(),
        cancel.clone(),
    )];
    tasks.extend(
        start_diagnostics_control(
            transport.clone(),
            id.clone(),
            diagnostic_state.clone(),
            diagnostics_control,
            enable_unauthenticated_diagnostics_mesh_control,
            cancel.clone(),
        )
        .await
        .map_err(|err| {
            log::error!("failed to start required diagnostics control channel: {err}");
            process::ExitCode::FAILURE
        })?,
    );

    for channel in ATAK_CHANNELS {
        let channel_tasks = start_bridge(
            transport.clone(),
            id.clone(),
            *channel,
            local_interface.clone(),
            location_state.clone(),
            diagnostic_state.clone(),
            allow_unvalidated_payloads,
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

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn configured_ipv4(
    cli_value: Option<Ipv4Addr>,
    env_name: &str,
) -> Result<Option<Ipv4Addr>, process::ExitCode> {
    if cli_value.is_some() {
        return Ok(cli_value);
    }
    match non_empty_env(env_name) {
        Some(value) => value.parse::<Ipv4Addr>().map(Some).map_err(|err| {
            log::error!("invalid {env_name} value '{value}': {err}");
            process::ExitCode::FAILURE
        }),
        None => Ok(None),
    }
}

fn configured_diagnostics_unix_socket(cli_value: Option<PathBuf>) -> Option<PathBuf> {
    let path = cli_value
        .or_else(|| non_empty_env(DIAGNOSTICS_UNIX_SOCKET_ENV).map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DIAGNOSTICS_UNIX_SOCKET));

    match validate_diagnostics_unix_socket_path(&path) {
        Ok(()) => Some(path),
        Err(err) => {
            log::warn!("diagnostics Unix socket disabled: {err}");
            None
        }
    }
}

fn validate_diagnostics_unix_socket_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("socket path is empty".to_string());
    }
    if !path.is_absolute() {
        return Err(format!("socket path must be absolute: {}", path.display()));
    }
    Ok(())
}

fn configured_diagnostics_udp_addr(
    cli_value: Option<SocketAddr>,
    allow_insecure: bool,
) -> Option<SocketAddr> {
    let configured = match cli_value {
        Some(value) => Some(value),
        None => match non_empty_env(DIAGNOSTICS_CONTROL_LISTEN_ENV) {
            Some(value) => match value.parse::<SocketAddr>() {
                Ok(parsed) => Some(parsed),
                Err(err) => {
                    log::warn!("diagnostics UDP control disabled: invalid {DIAGNOSTICS_CONTROL_LISTEN_ENV} value: {err}");
                    None
                }
            },
            None => None,
        },
    };

    configured.and_then(|listen_addr| {
        validate_diagnostics_control_listen(listen_addr, allow_insecure)
            .map(|_| listen_addr)
            .map_err(|err| {
                log::warn!("diagnostics UDP control disabled: {err}");
            })
            .ok()
    })
}

fn validate_diagnostics_control_listen(
    listen_addr: SocketAddr,
    allow_insecure: bool,
) -> Result<(), String> {
    if listen_addr.ip().is_loopback() || allow_insecure {
        return Ok(());
    }
    Err(format!(
        "diagnostics local control socket must bind to loopback unless \
         --allow-insecure-diagnostics-control-listen or \
         {INSECURE_DIAGNOSTICS_CONTROL_LISTEN_ENV}=true is set"
    ))
}

fn env_flag_enabled(name: &str) -> bool {
    non_empty_env(name)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
async fn start_bridge(
    transport: Arc<Mutex<Transport>>,
    identity: PrivateIdentity,
    channel: AtakChannel,
    local_interface: LocalInterface,
    location_state: Arc<LocationState>,
    diagnostic_state: Arc<DiagnosticState>,
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
        let diagnostic_state = diagnostic_state.clone();
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
                                    None,
                                    channel,
                                    &location_state,
                                    &diagnostic_state,
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
        let diagnostic_state = diagnostic_state.clone();
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
                                        log::info!("atak-plugin:{}:{} link activated", channel.name, channel.port);
                                    }
                                    LinkEvent::Closed => {
                                        peer_hashes.lock().await.remove(&ev.address_hash);
                                        log::info!("atak-plugin:{}:{} link closed", channel.name, channel.port);
                                    }
                                    LinkEvent::Data(payload) => {
                                        let data = payload.as_slice();
                                        let remote_peer_hash = ev.address_hash.to_string();
                                        if !accept_payload(
                                            data,
                                            PacketSource::RemoteReticulum,
                                            Some(&remote_peer_hash),
                                            channel,
                                            &location_state,
                                            &diagnostic_state,
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
                                let peer = ev.destination.lock().await.desc;
                                if peer.address_hash == dest_hash {
                                    continue;
                                }

                                peer_hashes.lock().await.insert(peer.address_hash);
                                log::info!("atak-plugin:{}:{} auto-link created", channel.name, channel.port);
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

async fn start_diagnostics_control(
    transport: Arc<Mutex<Transport>>,
    identity: PrivateIdentity,
    diagnostic_state: Arc<DiagnosticState>,
    control_config: DiagnosticsControlConfig,
    enable_mesh_control: bool,
    cancel: CancellationToken,
) -> Result<Vec<JoinHandle<()>>, Box<dyn std::error::Error + Send + Sync>> {
    let mut tasks = Vec::new();
    let mut local_control_available = false;
    #[cfg(unix)]
    let mut unix_control_socket = None;
    let mut udp_control_socket = None;

    #[cfg(unix)]
    if let Some(path) = control_config.unix_socket.as_deref() {
        match bind_unix_diagnostics_socket(path).await {
            Ok(control_socket) => {
                log::info!(
                    "atak-plugin:diagnostics local Unix socket listening at {}",
                    path.display()
                );
                local_control_available = true;
                unix_control_socket = Some(control_socket);
            }
            Err(err) => {
                let message = format!(
                    "diagnostics Unix socket {} unavailable: {err}",
                    path.display()
                );
                if control_config.require_control {
                    return Err(message.into());
                }
                log::warn!("{message}; ATAK forwarding will continue");
            }
        }
    }

    #[cfg(not(unix))]
    if control_config.unix_socket.is_some() {
        if control_config.require_control {
            return Err("diagnostics Unix socket requested on a non-Unix target".into());
        }
        log::warn!(
            "diagnostics Unix socket unavailable on this target; ATAK forwarding will continue"
        );
    }

    if let Some(local_control_addr) = control_config.udp_addr {
        match UdpSocket::bind(local_control_addr).await {
            Ok(control_socket) => {
                log::info!(
                    "atak-plugin:diagnostics compatibility UDP control listening on {}",
                    local_control_addr
                );
                local_control_available = true;
                udp_control_socket = Some(control_socket);
            }
            Err(err) => {
                let message =
                    format!("diagnostics UDP control {local_control_addr} unavailable: {err}");
                if control_config.require_control {
                    return Err(message.into());
                }
                log::warn!("{message}; ATAK forwarding will continue");
            }
        }
    }

    if !local_control_available {
        let message = "no local diagnostics control endpoint is available";
        if control_config.require_control {
            return Err(message.into());
        }
        log::warn!("{message}; ATAK forwarding will continue");
    }

    if !enable_mesh_control {
        #[cfg(unix)]
        if let Some(control_socket) = unix_control_socket {
            tasks.push(spawn_unix_diagnostics_control(
                control_socket,
                None,
                diagnostic_state.clone(),
                cancel.clone(),
            ));
        }
        if let Some(control_socket) = udp_control_socket {
            tasks.push(spawn_udp_diagnostics_control(
                control_socket,
                None,
                diagnostic_state,
                cancel,
            ));
        }
        log::info!(
            "atak-plugin:diagnostics mesh control disabled; no diagnostics Reticulum destination will be announced"
        );
        return Ok(tasks);
    }

    let peer_hashes = Arc::new(Mutex::new(HashSet::<AddressHash>::new()));
    let destination = transport
        .lock()
        .await
        .add_destination(
            identity,
            DestinationName::new("kaonic", DIAGNOSTICS_DEST_NAME),
        )
        .await;
    let dest_hash = destination.lock().await.desc.address_hash;

    if let Ok(pkt) = destination
        .lock()
        .await
        .announce(OsRng, Some(DIAGNOSTICS_PORT_TAG))
    {
        transport.lock().await.send_packet(pkt).await;
    }

    log::info!(
        "atak-plugin:diagnostics trusted-test mesh control destination active (disabled by default)"
    );

    #[cfg(unix)]
    if let Some(control_socket) = unix_control_socket {
        tasks.push(spawn_unix_diagnostics_control(
            control_socket,
            Some((transport.clone(), dest_hash)),
            diagnostic_state.clone(),
            cancel.clone(),
        ));
    }
    if let Some(control_socket) = udp_control_socket {
        tasks.push(spawn_udp_diagnostics_control(
            control_socket,
            Some((transport.clone(), dest_hash)),
            diagnostic_state.clone(),
            cancel.clone(),
        ));
    }

    let network_control = {
        let transport = transport.clone();
        let cancel = cancel.clone();
        let peer_hashes = peer_hashes.clone();
        let diagnostic_state = diagnostic_state.clone();
        tokio::spawn(async move {
            let mut events = transport.lock().await.out_link_events();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    result = events.recv() => {
                        match result {
                            Ok(ev) => {
                                if !peer_hashes.lock().await.contains(&ev.address_hash) {
                                    continue;
                                }
                                match ev.event {
                                    LinkEvent::Activated => {
                                        log::info!("atak-plugin:diagnostics control link activated");
                                    }
                                    LinkEvent::Closed => {
                                        peer_hashes.lock().await.remove(&ev.address_hash);
                                        log::info!("atak-plugin:diagnostics control link closed");
                                    }
                                    LinkEvent::Data(payload) => {
                                        match DiagnosticCommand::parse(payload.as_slice()) {
                                            Ok(command) if diagnostic_state.apply_once(&command) => {
                                                match command.action {
                                                    DiagnosticAction::Enable { seconds } => {
                                                        log::warn!("diagnostics peer-hash tracking enabled for {seconds}s by trusted-test mesh control message");
                                                    }
                                                    DiagnosticAction::Disable => {
                                                        log::warn!("diagnostics peer-hash tracking disabled by trusted-test mesh control message");
                                                    }
                                                }
                                                // Re-broadcast each new command once so it propagates through multi-hop plugin topologies.
                                                transport.lock().await.send_to_in_links(&dest_hash, &command.encode()).await;
                                            }
                                            Ok(_) => {}
                                            Err(err) => {
                                                log::warn!("dropping invalid diagnostics control message: {err}");
                                            }
                                        }
                                    }
                                    LinkEvent::Proof(_) => {}
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                log::warn!("atak-plugin:diagnostics skipped {skipped} Reticulum link events");
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
                                if ev.app_data.as_slice() != DIAGNOSTICS_PORT_TAG {
                                    continue;
                                }
                                let peer = ev.destination.lock().await.desc;
                                if peer.address_hash == dest_hash {
                                    continue;
                                }
                                peer_hashes.lock().await.insert(peer.address_hash);
                                log::info!("atak-plugin:diagnostics auto-link created");
                                transport.lock().await.link(peer).await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                log::warn!("atak-plugin:diagnostics skipped {skipped} Reticulum announces");
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
                        if let Ok(pkt) = destination.lock().await.announce(OsRng, Some(DIAGNOSTICS_PORT_TAG)) {
                            transport.lock().await.send_packet(pkt).await;
                        }
                    }
                }
            }
        })
    };

    tasks.push(network_control);
    tasks.push(auto_link);
    tasks.push(reannounce);

    Ok(tasks)
}

fn spawn_udp_diagnostics_control(
    control_socket: UdpSocket,
    mesh_control: Option<(Arc<Mutex<Transport>>, AddressHash)>,
    diagnostic_state: Arc<DiagnosticState>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0u8; MAX_LOCAL_DIAGNOSTIC_REQUEST_BYTES];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                result = control_socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, source)) => {
                            let response = handle_local_diagnostic_request(
                                &buf[..len],
                                mesh_control.as_ref(),
                                &diagnostic_state,
                            ).await;
                            if let Err(err) = control_socket.send_to(response.as_bytes(), source).await {
                                log::warn!("diagnostics local response error: {err}");
                            }
                        }
                        Err(err) => log::warn!("diagnostics local command socket error: {err}"),
                    }
                }
            }
        }
    })
}

#[cfg(unix)]
async fn bind_unix_diagnostics_socket(path: &Path) -> std::io::Result<UnixDatagram> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let socket = UnixDatagram::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(socket)
}

#[cfg(unix)]
fn spawn_unix_diagnostics_control(
    control_socket: UnixDatagram,
    mesh_control: Option<(Arc<Mutex<Transport>>, AddressHash)>,
    diagnostic_state: Arc<DiagnosticState>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0u8; MAX_LOCAL_DIAGNOSTIC_REQUEST_BYTES];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                result = control_socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, source)) => {
                            let response = handle_local_diagnostic_request(
                                &buf[..len],
                                mesh_control.as_ref(),
                                &diagnostic_state,
                            ).await;
                            if let Some(reply_path) = source.as_pathname() {
                                if let Err(err) = control_socket.send_to(response.as_bytes(), reply_path).await {
                                    log::warn!("diagnostics local Unix response error: {err}");
                                }
                            } else {
                                log::warn!("diagnostics local Unix request used an unnamed reply socket");
                            }
                        }
                        Err(err) => log::warn!("diagnostics local Unix socket error: {err}"),
                    }
                }
            }
        }
    })
}

fn spawn_diagnostics_expiry_cleanup(
    diagnostic_state: Arc<DiagnosticState>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {
                    diagnostic_state.prune_expired();
                }
            }
        }
    })
}

async fn handle_local_diagnostic_request(
    payload: &[u8],
    mesh_control: Option<&(Arc<Mutex<Transport>>, AddressHash)>,
    diagnostic_state: &DiagnosticState,
) -> String {
    match parse_local_diagnostic_request(payload) {
        Ok(LocalDiagnosticRequest::Enable(seconds)) => {
            match DiagnosticCommand::enable(new_command_id(), seconds) {
                Ok(command) => {
                    diagnostic_state.apply_once(&command);
                    if let Some((transport, dest_hash)) = mesh_control {
                        transport
                            .lock()
                            .await
                            .send_to_in_links(dest_hash, &command.encode())
                            .await;
                        log::warn!(
                            "diagnostics peer-hash tracking enabled locally for {seconds}s and announced across the diagnostic mesh"
                        );
                    } else {
                        log::warn!("diagnostics peer-hash tracking enabled locally for {seconds}s");
                    }
                    format_diagnostic_status(diagnostic_state)
                }
                Err(err) => format!("ERR {err}\n"),
            }
        }
        Ok(LocalDiagnosticRequest::Disable) => match DiagnosticCommand::disable(new_command_id()) {
            Ok(command) => {
                diagnostic_state.apply_once(&command);
                if let Some((transport, dest_hash)) = mesh_control {
                    transport
                        .lock()
                        .await
                        .send_to_in_links(dest_hash, &command.encode())
                        .await;
                    log::warn!(
                        "diagnostics peer-hash tracking disabled locally and announced across the diagnostic mesh"
                    );
                } else {
                    log::warn!("diagnostics peer-hash tracking disabled locally");
                }
                format_diagnostic_status(diagnostic_state)
            }
            Err(err) => format!("ERR {err}\n"),
        },
        Ok(LocalDiagnosticRequest::Status) => format_diagnostic_status(diagnostic_state),
        Ok(LocalDiagnosticRequest::Recent(limit)) => format_recent_records(diagnostic_state, limit),
        Err(err) => format!("ERR {err}\n"),
    }
}

#[allow(clippy::too_many_arguments)]
fn accept_payload(
    data: &[u8],
    source: PacketSource,
    remote_peer_hash: Option<&str>,
    channel: AtakChannel,
    location_state: &LocationState,
    diagnostic_state: &DiagnosticState,
    allow_unvalidated_payloads: bool,
    invalid_counter: &AtomicU64,
    location_counter: &AtomicU64,
) -> bool {
    match parse_cot_payload(data) {
        Ok(event) => {
            if let Some(peer_hash) = remote_peer_hash {
                if diagnostic_state.record_remote(peer_hash, channel.port, &event) {
                    log::debug!(
                        "diagnostics recorded remote CoT metadata on channel_port={}",
                        channel.port,
                    );
                }
            }
            if let Some(record) = location_state.record(source, channel.port, &event) {
                let known_locations = location_state.len();
                log::debug!(
                    "atak-plugin:{}:{} CoT source={:?} type={} recorded_port={} updated_at={:?} known_locations={}",
                    channel.name,
                    channel.port,
                    record.source,
                    record.event_type,
                    record.channel_port,
                    record.updated_at,
                    known_locations
                );
                location_counter.fetch_add(1, Ordering::Relaxed);
            } else {
                log::debug!(
                    "atak-plugin:{}:{} accepted valid non-location CoT type={} source={:?}",
                    channel.name,
                    channel.port,
                    event.event_type,
                    source
                );
            }
            true
        }
        Err(err) if allow_unvalidated_payloads => {
            invalid_counter.fetch_add(1, Ordering::Relaxed);
            log::debug!(
                "atak-plugin:{}:{} forwarding unvalidated payload by explicit override: {err}",
                channel.name,
                channel.port
            );
            true
        }
        Err(err) => {
            invalid_counter.fetch_add(1, Ordering::Relaxed);
            log::debug!(
                "atak-plugin:{}:{} dropping invalid ATAK payload: {err}",
                channel.name,
                channel.port
            );
            false
        }
    }
}

fn new_command_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    let mut rng = OsRng;
    let random = rng.next_u64();
    format!("{timestamp:x}-{random:x}")
}

fn parse_local_diagnostic_request(payload: &[u8]) -> Result<LocalDiagnosticRequest, String> {
    if payload.len() >= MAX_LOCAL_DIAGNOSTIC_REQUEST_BYTES || payload.len() > MAX_COMMAND_BYTES {
        return Err("local diagnostics command is too large".to_string());
    }
    let text = std::str::from_utf8(payload)
        .map_err(|_| "local diagnostics command is not UTF-8".to_string())?;
    let fields: Vec<&str> = text.split_whitespace().collect();
    match fields.as_slice() {
        ["enable"] => Ok(LocalDiagnosticRequest::Enable(DEFAULT_ENABLE_SECONDS)),
        ["enable", seconds] => {
            let seconds = seconds
                .parse::<u64>()
                .map_err(|_| "enable duration must be an integer number of seconds".to_string())?;
            if !(1..=MAX_ENABLE_SECONDS).contains(&seconds) {
                return Err(format!(
                    "enable duration must be between 1 and {MAX_ENABLE_SECONDS} seconds"
                ));
            }
            Ok(LocalDiagnosticRequest::Enable(seconds))
        }
        ["disable"] => Ok(LocalDiagnosticRequest::Disable),
        ["status"] => Ok(LocalDiagnosticRequest::Status),
        ["recent"] => Ok(LocalDiagnosticRequest::Recent(10)),
        ["recent", limit] => {
            let limit = limit
                .parse::<usize>()
                .map_err(|_| "recent limit must be an integer".to_string())?;
            if !(1..=MAX_LOCAL_RECENT_RECORDS).contains(&limit) {
                return Err(format!(
                    "recent limit must be between 1 and {MAX_LOCAL_RECENT_RECORDS}"
                ));
            }
            Ok(LocalDiagnosticRequest::Recent(limit))
        }
        _ => Err("expected: enable [seconds], disable, status, or recent [1-20]".to_string()),
    }
}

fn format_diagnostic_status(diagnostic_state: &DiagnosticState) -> String {
    let status = diagnostic_state.status();
    format!(
        "OK enabled={} remaining_seconds={} records={}\n",
        status.enabled, status.remaining_seconds, status.record_count
    )
}

fn format_recent_records(diagnostic_state: &DiagnosticState, limit: usize) -> String {
    let records = diagnostic_state.recent(limit);
    let mut output = format_diagnostic_status(diagnostic_state);
    for record in records {
        output.push_str(&format_diagnostic_record(&record));
    }
    output
}

fn format_diagnostic_record(record: &DiagnosticRecord) -> String {
    let observed_unix_ms = record
        .observed_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis();
    let callsign = sanitize_field(record.callsign.as_deref().unwrap_or("-"));
    let uid = sanitize_field(&record.uid);
    let event_type = sanitize_field(&record.event_type);
    match record.point {
        Some((lat, lon)) => format!(
            "RECORD unix_ms={} peer={} port={} uid={} callsign={} type={} lat={} lon={}\n",
            observed_unix_ms,
            record.remote_peer_hash,
            record.channel_port,
            uid,
            callsign,
            event_type,
            lat,
            lon
        ),
        None => format!(
            "RECORD unix_ms={} peer={} port={} uid={} callsign={} type={} lat=- lon=-\n",
            observed_unix_ms,
            record.remote_peer_hash,
            record.channel_port,
            uid,
            callsign,
            event_type
        ),
    }
}

fn sanitize_field(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_graphic() && character != '=' {
                character
            } else {
                '_'
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_diagnostics_requests() {
        assert!(matches!(
            parse_local_diagnostic_request(b"enable 120").unwrap(),
            LocalDiagnosticRequest::Enable(120)
        ));
        assert!(matches!(
            parse_local_diagnostic_request(b"disable").unwrap(),
            LocalDiagnosticRequest::Disable
        ));
        assert!(parse_local_diagnostic_request(b"enable 0").is_err());
        assert!(parse_local_diagnostic_request(b"recent 21").is_err());
        assert!(parse_local_diagnostic_request(&[0xff]).is_err());
        assert!(
            parse_local_diagnostic_request(&vec![b'a'; MAX_LOCAL_DIAGNOSTIC_REQUEST_BYTES])
                .is_err()
        );
    }

    #[test]
    fn diagnostics_control_listen_requires_loopback_without_override() {
        let loopback: SocketAddr = "127.0.0.1:19001".parse().unwrap();
        let non_loopback: SocketAddr = "0.0.0.0:19001".parse().unwrap();
        assert!(validate_diagnostics_control_listen(loopback, false).is_ok());
        assert!(validate_diagnostics_control_listen(non_loopback, false).is_err());
        assert!(validate_diagnostics_control_listen(non_loopback, true).is_ok());
    }

    #[test]
    fn diagnostics_unix_socket_path_must_be_absolute() {
        assert!(validate_diagnostics_unix_socket_path(Path::new(
            "/run/kaonic-atak-plugin/diagnostics.sock"
        ))
        .is_ok());
        assert!(validate_diagnostics_unix_socket_path(Path::new("diagnostics.sock")).is_err());
    }

    #[tokio::test]
    async fn local_diagnostics_control_does_not_relay_without_mesh_control() {
        let state = DiagnosticState::default();
        let response = handle_local_diagnostic_request(b"enable 120", None, &state).await;
        assert!(response.starts_with("OK enabled=true"));
        assert!(state.status().enabled);
    }
}
