use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::cot::CotEvent;

pub const DEFAULT_ENABLE_SECONDS: u64 = 15 * 60;
pub const MAX_ENABLE_SECONDS: u64 = 24 * 60 * 60;
pub const MAX_COMMAND_BYTES: usize = 512;
const CONTROL_VERSION: &str = "KAD1";
const DEFAULT_MAX_RECORDS: usize = 512;
const DEFAULT_MAX_SEEN_COMMANDS: usize = 2048;
const SEEN_COMMAND_RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticAction {
    Enable { seconds: u64 },
    Disable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticCommand {
    pub id: String,
    pub action: DiagnosticAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticCommandError {
    NotUtf8,
    InvalidFormat,
    InvalidVersion,
    InvalidId,
    InvalidDuration,
    InvalidAction,
    TooLarge,
}

impl fmt::Display for DiagnosticCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => write!(f, "diagnostics command is not UTF-8"),
            Self::InvalidFormat => write!(f, "invalid diagnostics command format"),
            Self::InvalidVersion => write!(f, "unsupported diagnostics command version"),
            Self::InvalidId => write!(f, "invalid diagnostics command id"),
            Self::InvalidDuration => write!(f, "invalid diagnostics enable duration"),
            Self::InvalidAction => write!(f, "invalid diagnostics command action"),
            Self::TooLarge => write!(f, "diagnostics command is too large"),
        }
    }
}

impl std::error::Error for DiagnosticCommandError {}

impl DiagnosticCommand {
    pub fn enable(id: String, seconds: u64) -> Result<Self, DiagnosticCommandError> {
        validate_id(&id)?;
        if !(1..=MAX_ENABLE_SECONDS).contains(&seconds) {
            return Err(DiagnosticCommandError::InvalidDuration);
        }
        Ok(Self {
            id,
            action: DiagnosticAction::Enable { seconds },
        })
    }

    pub fn disable(id: String) -> Result<Self, DiagnosticCommandError> {
        validate_id(&id)?;
        Ok(Self {
            id,
            action: DiagnosticAction::Disable,
        })
    }

    pub fn parse(payload: &[u8]) -> Result<Self, DiagnosticCommandError> {
        if payload.len() > MAX_COMMAND_BYTES {
            return Err(DiagnosticCommandError::TooLarge);
        }
        let text = std::str::from_utf8(payload).map_err(|_| DiagnosticCommandError::NotUtf8)?;
        let fields: Vec<&str> = text.trim().split('|').collect();
        if fields.len() != 4 {
            return Err(DiagnosticCommandError::InvalidFormat);
        }
        if fields[0] != CONTROL_VERSION {
            return Err(DiagnosticCommandError::InvalidVersion);
        }
        let id = fields[1].to_string();
        match fields[2] {
            "enable" => {
                let seconds = fields[3]
                    .parse::<u64>()
                    .map_err(|_| DiagnosticCommandError::InvalidDuration)?;
                Self::enable(id, seconds)
            }
            "disable" if fields[3] == "0" => Self::disable(id),
            "disable" => Err(DiagnosticCommandError::InvalidDuration),
            _ => Err(DiagnosticCommandError::InvalidAction),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let text = match self.action {
            DiagnosticAction::Enable { seconds } => {
                format!("{CONTROL_VERSION}|{}|enable|{seconds}", self.id)
            }
            DiagnosticAction::Disable => format!("{CONTROL_VERSION}|{}|disable|0", self.id),
        };
        text.into_bytes()
    }
}

fn validate_id(id: &str) -> Result<(), DiagnosticCommandError> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(DiagnosticCommandError::InvalidId);
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct DiagnosticRecord {
    pub remote_peer_hash: String,
    pub channel_port: u16,
    pub uid: String,
    pub callsign: Option<String>,
    pub event_type: String,
    pub point: Option<(f64, f64)>,
    pub observed_at: SystemTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticStatus {
    pub enabled: bool,
    pub remaining_seconds: u64,
    pub record_count: usize,
}

struct DiagnosticInner {
    enabled_until: Option<SystemTime>,
    records: VecDeque<DiagnosticRecord>,
    seen_commands: HashMap<String, SystemTime>,
    seen_order: VecDeque<String>,
}

pub struct DiagnosticState {
    inner: Mutex<DiagnosticInner>,
    max_records: usize,
    max_seen_commands: usize,
}

impl Default for DiagnosticState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_RECORDS)
    }
}

impl DiagnosticState {
    pub fn new(max_records: usize) -> Self {
        Self {
            inner: Mutex::new(DiagnosticInner {
                enabled_until: None,
                records: VecDeque::new(),
                seen_commands: HashMap::new(),
                seen_order: VecDeque::new(),
            }),
            max_records: max_records.max(1),
            max_seen_commands: DEFAULT_MAX_SEEN_COMMANDS,
        }
    }

    /// Applies a network command once. Returning false means this command was
    /// already observed and should not be re-broadcast across the mesh.
    pub fn apply_once(&self, command: &DiagnosticCommand) -> bool {
        self.apply_once_at(command, SystemTime::now())
    }

    fn apply_once_at(&self, command: &DiagnosticCommand, now: SystemTime) -> bool {
        let mut inner = self.inner.lock().expect("diagnostics state poisoned");
        self.prune_locked(&mut inner, now);
        if inner.seen_commands.contains_key(&command.id) {
            return false;
        }
        inner.seen_commands.insert(command.id.clone(), now);
        inner.seen_order.push_back(command.id.clone());
        self.enforce_seen_bound_locked(&mut inner);
        match command.action {
            DiagnosticAction::Enable { seconds } => {
                inner.enabled_until = now.checked_add(Duration::from_secs(seconds));
            }
            DiagnosticAction::Disable => {
                inner.enabled_until = None;
                inner.records.clear();
            }
        }
        true
    }

    /// Records valid remote CoT metadata only when diagnostics have been enabled.
    /// ATAK packet bytes are never altered by this state.
    pub fn record_remote(&self, peer_hash: &str, channel_port: u16, event: &CotEvent) -> bool {
        self.record_remote_at(peer_hash, channel_port, event, SystemTime::now())
    }

    fn record_remote_at(
        &self,
        peer_hash: &str,
        channel_port: u16,
        event: &CotEvent,
        now: SystemTime,
    ) -> bool {
        let mut inner = self.inner.lock().expect("diagnostics state poisoned");
        self.prune_locked(&mut inner, now);
        if !is_enabled_locked(&inner, now) {
            return false;
        }
        inner.records.push_back(DiagnosticRecord {
            remote_peer_hash: peer_hash.to_string(),
            channel_port,
            uid: event.uid.clone(),
            callsign: event.callsign.clone(),
            event_type: event.event_type.clone(),
            point: event.point.as_ref().map(|point| (point.lat, point.lon)),
            observed_at: now,
        });
        while inner.records.len() > self.max_records {
            inner.records.pop_front();
        }
        true
    }

    pub fn status(&self) -> DiagnosticStatus {
        self.status_at(SystemTime::now())
    }

    fn status_at(&self, now: SystemTime) -> DiagnosticStatus {
        let mut inner = self.inner.lock().expect("diagnostics state poisoned");
        self.prune_locked(&mut inner, now);
        let remaining_seconds = inner
            .enabled_until
            .and_then(|until| until.duration_since(now).ok())
            .map(|remaining| remaining.as_secs())
            .unwrap_or(0);
        DiagnosticStatus {
            enabled: remaining_seconds > 0,
            remaining_seconds,
            record_count: inner.records.len(),
        }
    }

    pub fn recent(&self, limit: usize) -> Vec<DiagnosticRecord> {
        self.recent_at(limit, SystemTime::now())
    }

    fn recent_at(&self, limit: usize, now: SystemTime) -> Vec<DiagnosticRecord> {
        let mut inner = self.inner.lock().expect("diagnostics state poisoned");
        self.prune_locked(&mut inner, now);
        inner.records.iter().rev().take(limit).cloned().collect()
    }

    fn prune_locked(&self, inner: &mut DiagnosticInner, now: SystemTime) {
        if inner
            .enabled_until
            .map(|until| until <= now)
            .unwrap_or(false)
        {
            inner.enabled_until = None;
            inner.records.clear();
        }
        while let Some(command_id) = inner.seen_order.front() {
            let keep = inner
                .seen_commands
                .get(command_id)
                .map(|observed_at| {
                    now.duration_since(*observed_at).unwrap_or(Duration::ZERO)
                        <= SEEN_COMMAND_RETENTION
                })
                .unwrap_or(false);
            if keep {
                break;
            }
            if let Some(command_id) = inner.seen_order.pop_front() {
                inner.seen_commands.remove(&command_id);
            }
        }
    }

    fn enforce_seen_bound_locked(&self, inner: &mut DiagnosticInner) {
        while inner.seen_commands.len() > self.max_seen_commands {
            if let Some(command_id) = inner.seen_order.pop_front() {
                inner.seen_commands.remove(&command_id);
            } else {
                break;
            }
        }
    }

    #[cfg(test)]
    fn seen_command_count(&self) -> usize {
        self.inner
            .lock()
            .expect("diagnostics state poisoned")
            .seen_commands
            .len()
    }
}

fn is_enabled_locked(inner: &DiagnosticInner, now: SystemTime) -> bool {
    inner
        .enabled_until
        .map(|until| until > now)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cot::{CotEvent, CotPoint};

    fn location_event() -> CotEvent {
        CotEvent {
            uid: "ANDROID-123".to_string(),
            event_type: "a-f-G-U-C".to_string(),
            how: Some("m-g".to_string()),
            callsign: Some("PHONE-A".to_string()),
            time: None,
            start: None,
            stale: None,
            point: Some(CotPoint {
                lat: 38.8977,
                lon: -77.0365,
                hae: None,
                ce: None,
                le: None,
            }),
        }
    }

    #[test]
    fn parses_enable_and_disable_commands() {
        let enable = DiagnosticCommand::enable("abc-123".to_string(), 900).unwrap();
        assert_eq!(DiagnosticCommand::parse(&enable.encode()).unwrap(), enable);
        let disable = DiagnosticCommand::disable("abc-124".to_string()).unwrap();
        assert_eq!(
            DiagnosticCommand::parse(&disable.encode()).unwrap(),
            disable
        );
    }

    #[test]
    fn rejects_invalid_control_commands() {
        assert_eq!(
            DiagnosticCommand::parse(b"KAD2|abc-123|enable|900").unwrap_err(),
            DiagnosticCommandError::InvalidVersion
        );
        assert_eq!(
            DiagnosticCommand::parse(b"KAD1|bad_id|enable|900").unwrap_err(),
            DiagnosticCommandError::InvalidId
        );
        assert_eq!(
            DiagnosticCommand::parse(b"KAD1|abc-123|enable|0").unwrap_err(),
            DiagnosticCommandError::InvalidDuration
        );
        assert_eq!(
            DiagnosticCommand::parse(b"KAD1|abc-123|disable|1").unwrap_err(),
            DiagnosticCommandError::InvalidDuration
        );
        assert_eq!(
            DiagnosticCommand::parse(b"KAD1|abc-123|enable").unwrap_err(),
            DiagnosticCommandError::InvalidFormat
        );
        assert_eq!(
            DiagnosticCommand::parse(&[0xff]).unwrap_err(),
            DiagnosticCommandError::NotUtf8
        );
        assert_eq!(
            DiagnosticCommand::parse(&vec![b'a'; MAX_COMMAND_BYTES + 1]).unwrap_err(),
            DiagnosticCommandError::TooLarge
        );
    }

    #[test]
    fn suppresses_duplicate_control_commands() {
        let state = DiagnosticState::default();
        let command = DiagnosticCommand::enable("abc-123".to_string(), 900).unwrap();
        assert!(state.apply_once(&command));
        assert!(!state.apply_once(&command));
    }

    #[test]
    fn records_peer_association_only_while_enabled() {
        let state = DiagnosticState::default();
        let event = location_event();
        assert!(!state.record_remote("peer-a", 6969, &event));
        state.apply_once(&DiagnosticCommand::enable("enable-1".to_string(), 900).unwrap());
        assert!(state.record_remote("peer-a", 6969, &event));
        assert_eq!(state.recent(10).len(), 1);
        state.apply_once(&DiagnosticCommand::disable("disable-1".to_string()).unwrap());
        assert!(!state.record_remote("peer-a", 6969, &event));
    }

    #[test]
    fn clears_records_on_disable() {
        let state = DiagnosticState::default();
        let event = location_event();
        state.apply_once(&DiagnosticCommand::enable("enable-1".to_string(), 900).unwrap());
        assert!(state.record_remote("peer-a", 6969, &event));
        state.apply_once(&DiagnosticCommand::disable("disable-1".to_string()).unwrap());
        assert_eq!(state.status().record_count, 0);
        assert!(state.recent(10).is_empty());
    }

    #[test]
    fn clears_records_on_expiry() {
        let state = DiagnosticState::default();
        let event = location_event();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        state.apply_once_at(
            &DiagnosticCommand::enable("enable-1".to_string(), 5).unwrap(),
            now,
        );
        assert!(state.record_remote_at("peer-a", 6969, &event, now + Duration::from_secs(1)));
        let status = state.status_at(now + Duration::from_secs(6));
        assert!(!status.enabled);
        assert_eq!(status.record_count, 0);
    }

    #[test]
    fn bounds_records_and_seen_commands() {
        let state = DiagnosticState::new(1);
        let event = location_event();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        state.apply_once_at(
            &DiagnosticCommand::enable("enable-1".to_string(), 900).unwrap(),
            now,
        );
        assert!(state.record_remote_at("peer-a", 6969, &event, now));
        assert!(state.record_remote_at("peer-b", 6969, &event, now));
        assert_eq!(state.recent_at(10, now).len(), 1);

        for index in 0..(DEFAULT_MAX_SEEN_COMMANDS + 2) {
            let command =
                DiagnosticCommand::disable(format!("disable-{index}")).expect("valid command");
            state.apply_once_at(&command, now + Duration::from_secs(index as u64));
        }
        assert!(state.seen_command_count() <= DEFAULT_MAX_SEEN_COMMANDS);
    }
}
