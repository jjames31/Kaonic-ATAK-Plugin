use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

use crate::interface::LocalInterface;

#[derive(Debug, Clone, Copy)]
pub struct AtakChannel {
    pub name: &'static str,
    pub group: Ipv4Addr,
    pub port: u16,
}

pub const ATAK_CHANNELS: &[AtakChannel] = &[
    AtakChannel {
        name: "cot",
        group: Ipv4Addr::new(239, 2, 3, 1),
        port: 6969,
    },
    AtakChannel {
        name: "geochat",
        group: Ipv4Addr::new(224, 10, 10, 1),
        port: 17012,
    },
];

pub struct MulticastSockets {
    pub receiver: Arc<UdpSocket>,
    pub sender: Arc<UdpSocket>,
    pub target: SocketAddr,
}

pub fn open_multicast_sockets(
    channel: AtakChannel,
    local_interface: &LocalInterface,
) -> io::Result<MulticastSockets> {
    let receiver = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    receiver.set_reuse_address(true)?;
    receiver.set_nonblocking(true)?;
    receiver.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, channel.port).into())?;
    receiver.join_multicast_v4(&channel.group, &local_interface.addr)?;

    let sender = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sender.set_nonblocking(true)?;
    sender.set_multicast_loop_v4(false)?;
    sender.set_multicast_ttl_v4(1)?;
    sender.set_multicast_if_v4(&local_interface.addr)?;
    sender.bind(&SocketAddrV4::new(local_interface.addr, 0).into())?;

    Ok(MulticastSockets {
        receiver: Arc::new(UdpSocket::from_std(receiver.into())?),
        sender: Arc::new(UdpSocket::from_std(sender.into())?),
        target: SocketAddrV4::new(channel.group, channel.port).into(),
    })
}
