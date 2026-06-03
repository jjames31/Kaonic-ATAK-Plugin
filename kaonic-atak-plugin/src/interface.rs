use std::fmt;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCandidate {
    pub name: String,
    pub addr: Ipv4Addr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalInterface {
    pub name: String,
    pub addr: Ipv4Addr,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterfaceSelection {
    pub interface_name: Option<String>,
    pub local_addr: Option<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceSelectionError {
    NoUsableInterfaces,
    NoAutomaticAtakInterface,
    InterfaceNotFound(String),
    AddressNotFound(Ipv4Addr),
    AmbiguousInterface(String),
}

impl fmt::Display for InterfaceSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoUsableInterfaces => write!(f, "no non-loopback IPv4 interfaces found"),
            Self::NoAutomaticAtakInterface => write!(
                f,
                "no unambiguous 192.168.10.0/24 ATAK interface found; configure KAONIC_ATAK_INTERFACE_IP or --local-address explicitly"
            ),
            Self::InterfaceNotFound(name) => write!(f, "interface '{name}' was not found"),
            Self::AddressNotFound(addr) => write!(f, "local address {addr} was not found"),
            Self::AmbiguousInterface(detail) => {
                write!(f, "local ATAK interface selection is ambiguous: {detail}")
            }
        }
    }
}

impl std::error::Error for InterfaceSelectionError {}

pub fn load_interface_candidates() -> Vec<InterfaceCandidate> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.addr.ip() {
            IpAddr::V4(addr) if !addr.is_loopback() => Some(InterfaceCandidate {
                name: iface.name,
                addr,
            }),
            _ => None,
        })
        .collect()
}

pub fn select_local_interface(
    candidates: &[InterfaceCandidate],
    selection: &InterfaceSelection,
) -> Result<LocalInterface, InterfaceSelectionError> {
    let candidates = candidates
        .iter()
        .filter(
            |candidate| match (&selection.interface_name, selection.local_addr) {
                (Some(name), Some(addr)) => candidate.name == *name && candidate.addr == addr,
                (Some(name), None) => candidate.name == *name,
                (None, Some(addr)) => candidate.addr == addr,
                (None, None) => true,
            },
        )
        .cloned()
        .collect::<Vec<_>>();

    if let Some(name) = &selection.interface_name {
        if candidates.is_empty() {
            return Err(InterfaceSelectionError::InterfaceNotFound(name.clone()));
        }
    }

    if let Some(addr) = selection.local_addr {
        if candidates.is_empty() {
            return Err(InterfaceSelectionError::AddressNotFound(addr));
        }
    }

    match (
        selection.interface_name.is_some(),
        selection.local_addr.is_some(),
    ) {
        (true, true) => one_candidate(candidates, "explicit interface/address"),
        (true, false) => one_candidate(candidates, "explicit interface"),
        (false, true) => one_candidate(candidates, "explicit address"),
        (false, false) => auto_detect(candidates),
    }
}

fn auto_detect(
    candidates: Vec<InterfaceCandidate>,
) -> Result<LocalInterface, InterfaceSelectionError> {
    if candidates.is_empty() {
        return Err(InterfaceSelectionError::NoUsableInterfaces);
    }

    let kaonic_lan = candidates
        .iter()
        .filter(|candidate| is_kaonic_lan_addr(candidate.addr))
        .cloned()
        .collect::<Vec<_>>();

    match kaonic_lan.as_slice() {
        [candidate] => Ok(to_local_interface(candidate)),
        [] => Err(InterfaceSelectionError::NoAutomaticAtakInterface),
        _ => Err(InterfaceSelectionError::AmbiguousInterface(format!(
            "multiple 192.168.10.0/24 candidates: {}",
            describe_candidates(&kaonic_lan)
        ))),
    }
}

fn one_candidate(
    candidates: Vec<InterfaceCandidate>,
    context: &str,
) -> Result<LocalInterface, InterfaceSelectionError> {
    match candidates.as_slice() {
        [] => Err(InterfaceSelectionError::NoUsableInterfaces),
        [candidate] => Ok(to_local_interface(candidate)),
        _ => Err(InterfaceSelectionError::AmbiguousInterface(format!(
            "{context} matched {}",
            describe_candidates(&candidates)
        ))),
    }
}

fn to_local_interface(candidate: &InterfaceCandidate) -> LocalInterface {
    LocalInterface {
        name: candidate.name.clone(),
        addr: candidate.addr,
    }
}

fn is_kaonic_lan_addr(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    octets[0] == 192 && octets[1] == 168 && octets[2] == 10
}

fn describe_candidates(candidates: &[InterfaceCandidate]) -> String {
    candidates
        .iter()
        .map(|candidate| format!("{}={}", candidate.name, candidate.addr))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, addr: [u8; 4]) -> InterfaceCandidate {
        InterfaceCandidate {
            name: name.into(),
            addr: Ipv4Addr::from(addr),
        }
    }

    #[test]
    fn explicit_interface_selects_single_address() {
        let candidates = vec![
            candidate("usb0", [192, 168, 10, 2]),
            candidate("wlan0", [10, 0, 0, 2]),
        ];

        let selected = select_local_interface(
            &candidates,
            &InterfaceSelection {
                interface_name: Some("usb0".into()),
                local_addr: None,
            },
        )
        .expect("selected interface");

        assert_eq!(selected.name, "usb0");
        assert_eq!(selected.addr, Ipv4Addr::new(192, 168, 10, 2));
    }

    #[test]
    fn explicit_non_default_address_is_permitted() {
        let candidates = vec![candidate("eth0", [10, 1, 2, 3])];
        let selected = select_local_interface(
            &candidates,
            &InterfaceSelection {
                interface_name: None,
                local_addr: Some(Ipv4Addr::new(10, 1, 2, 3)),
            },
        )
        .expect("explicit address is safe");
        assert_eq!(selected.name, "eth0");
    }

    #[test]
    fn auto_detect_prefers_single_kaonic_lan_candidate() {
        let candidates = vec![
            candidate("eth0", [10, 0, 0, 2]),
            candidate("usb0", [192, 168, 10, 2]),
        ];

        let selected = select_local_interface(&candidates, &InterfaceSelection::default())
            .expect("selected interface");

        assert_eq!(selected.name, "usb0");
    }

    #[test]
    fn auto_detect_refuses_unrelated_single_interface() {
        let candidates = vec![candidate("eth0", [10, 0, 0, 2])];
        assert_eq!(
            select_local_interface(&candidates, &InterfaceSelection::default()).unwrap_err(),
            InterfaceSelectionError::NoAutomaticAtakInterface
        );
    }

    #[test]
    fn auto_detect_rejects_ambiguous_interfaces() {
        let candidates = vec![
            candidate("usb0", [192, 168, 10, 2]),
            candidate("wlan0", [192, 168, 10, 3]),
        ];

        assert!(matches!(
            select_local_interface(&candidates, &InterfaceSelection::default()),
            Err(InterfaceSelectionError::AmbiguousInterface(_))
        ));
    }

    #[test]
    fn explicit_interface_rejects_multiple_addresses_without_address() {
        let candidates = vec![
            candidate("usb0", [192, 168, 10, 2]),
            candidate("usb0", [10, 0, 0, 2]),
        ];

        assert!(matches!(
            select_local_interface(
                &candidates,
                &InterfaceSelection {
                    interface_name: Some("usb0".into()),
                    local_addr: None,
                },
            ),
            Err(InterfaceSelectionError::AmbiguousInterface(_))
        ));
    }
}
