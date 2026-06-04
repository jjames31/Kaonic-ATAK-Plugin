use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::cot::CotEvent;

pub const DEFAULT_ENABLE_SECONDS: u64 = 15 * 60;
pub const MAX_ENABLE_SECONDS: u64 = 24 * 60 * 60;
const CONTROL_VERSION: &str = "KAD1";
const DEFAULT_MAX_RECORDS: usize = 512;
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
}

pub struct DiagnosticState {
    inner: Mutex<DiagnosticInner>,
    max_records: usize,
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
            }),
            max_records: max_records.max(1),
        }
    }

    /// Applies a network command once. Returning false means this command was
    /// already observed and should not be re-broadcast across the mesh.
    pub fn apply_once(&self, command: &DiagnosticCommand) -> bool {
        let now = SystemTime::now();
        let mut inner = self.inner.lock().expect("diagnostics state poisoned");
        self.prune_locked(&mut inner, now);
        if inner.seen_commands.contains_key(&command.id) {
            return false;
        }
        inner.seen_commands.insert(command.id.clone(), now);
        match command.action {
            DiagnosticAction::Enable { seconds } => {
                inner.enabled_until = now.checked_add(Duration::from_secs(seconds));
            }
            DiagnosticAction::Disable => {
                inner.enabled_until = None;
            }
        }
        true
    }

    /// Records valid remote CoT metadata only when diagnostics have been enabled.
    /// ATAK packet bytes are never altered by this state.
    pub fn record_remote(&self, peer_hash: &str, channel_port: u16, event: &CotEvent) -> bool {
        let now = SystemTime::now();
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
        let now = SystemTime::now();
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
        let now = SystemTime::now();
        let mut inner = self.inner.lock().expect("diagnostics state poisoned");
        self.prune_locked(&mut inner, now);
        inner
            .records
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    fn prune_locked(&self, inner: &mut DiagnosticInner, now: SystemTime) {
        if inner
            .enabled_until
            .map(|until| until <= now)
            .unwrap_or(false)
        {
            inner.enabled_until = None;
        }
        inner.seen_commands.retain(|_, observed_at| {
            now.duration_since(*observed_at).unwrap_or(Duration::ZERO) <= SEEN_COMMAND_RETENTION
        });
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
        assert_eq!(DiagnosticCommand::parse(&disable.encode()).unwrap(), disable);
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
        state
            .apply_once(&DiagnosticCommand::enable("enable-1".to_string(), 900).unwrap());
        assert!(state.record_remote("peer-a", 6969, &event));
        assert_eq!(state.recent(10).len(), 1);
        state
            .apply_once(&DiagnosticCommand::disable("disable-1".to_string()).unwrap());
        assert!(!state.record_remote("peer-a", 6969, &event));
    }
}
