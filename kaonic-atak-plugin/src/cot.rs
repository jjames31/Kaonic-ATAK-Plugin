use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const DEFAULT_MAX_LOCATION_RECORDS: usize = 512;
const DEFAULT_RECORD_RETENTION: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq)]
pub struct CotPoint {
    pub lat: f64,
    pub lon: f64,
    pub hae: Option<f64>,
    pub ce: Option<f64>,
    pub le: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CotEvent {
    pub uid: String,
    pub event_type: String,
    pub how: Option<String>,
    pub callsign: Option<String>,
    pub time: Option<String>,
    pub start: Option<String>,
    pub stale: Option<String>,
    pub point: Option<CotPoint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CotParseError {
    NotUtf8,
    Xml(String),
    WrongRoot,
    MissingAttribute(&'static str),
    InvalidNumber(&'static str),
    InvalidCoordinate(&'static str),
}

impl fmt::Display for CotParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUtf8 => write!(f, "payload is not UTF-8"),
            Self::Xml(err) => write!(f, "invalid XML: {err}"),
            Self::WrongRoot => write!(f, "root element is not CoT event"),
            Self::MissingAttribute(attr) => write!(f, "missing CoT attribute {attr}"),
            Self::InvalidNumber(attr) => write!(f, "invalid CoT numeric attribute {attr}"),
            Self::InvalidCoordinate(attr) => write!(f, "invalid CoT coordinate {attr}"),
        }
    }
}

impl std::error::Error for CotParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacketSource {
    LocalUdp,
    RemoteReticulum,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LocationRecord {
    pub uid: String,
    pub event_type: String,
    pub how: Option<String>,
    pub callsign: Option<String>,
    pub point: CotPoint,
    pub source: PacketSource,
    pub channel_port: u16,
    pub updated_at: SystemTime,
}

pub struct LocationState {
    records: Mutex<HashMap<(PacketSource, String), LocationRecord>>,
    max_records: usize,
    retention: Duration,
}

impl Default for LocationState {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_LOCATION_RECORDS, DEFAULT_RECORD_RETENTION)
    }
}

impl LocationState {
    pub fn new(max_records: usize, retention: Duration) -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            max_records: max_records.max(1),
            retention,
        }
    }

    pub fn record(
        &self,
        source: PacketSource,
        channel_port: u16,
        event: &CotEvent,
    ) -> Option<LocationRecord> {
        let point = event.point.clone()?;
        let record = LocationRecord {
            uid: event.uid.clone(),
            event_type: event.event_type.clone(),
            how: event.how.clone(),
            callsign: event.callsign.clone(),
            point,
            source,
            channel_port,
            updated_at: SystemTime::now(),
        };

        let mut records = self.records.lock().expect("location state poisoned");
        self.prune_locked(&mut records);
        records.insert((source, record.uid.clone()), record.clone());
        while records.len() > self.max_records {
            if let Some(oldest_key) = records
                .iter()
                .min_by_key(|(_, stored)| stored.updated_at)
                .map(|(key, _)| key.clone())
            {
                records.remove(&oldest_key);
            } else {
                break;
            }
        }
        Some(record)
    }

    pub fn len(&self) -> usize {
        let mut records = self.records.lock().expect("location state poisoned");
        self.prune_locked(&mut records);
        records.len()
    }

    fn prune_locked(&self, records: &mut HashMap<(PacketSource, String), LocationRecord>) {
        let now = SystemTime::now();
        records.retain(|_, record| {
            now.duration_since(record.updated_at)
                .unwrap_or(Duration::ZERO)
                <= self.retention
        });
    }

    #[cfg(test)]
    pub fn get(&self, source: PacketSource, uid: &str) -> Option<LocationRecord> {
        self.records
            .lock()
            .expect("location state poisoned")
            .get(&(source, uid.to_string()))
            .cloned()
    }
}

/// Validates a UTF-8 Cursor-on-Target event and extracts a position only when it
/// has a usable point. A valid non-location event is still safe to forward.
pub fn parse_cot_payload(payload: &[u8]) -> Result<CotEvent, CotParseError> {
    let xml = std::str::from_utf8(payload).map_err(|_| CotParseError::NotUtf8)?;
    let doc = roxmltree::Document::parse(xml).map_err(|err| CotParseError::Xml(err.to_string()))?;
    let event = doc.root_element();
    if event.tag_name().name() != "event" {
        return Err(CotParseError::WrongRoot);
    }

    let uid = required_attr(event, "uid")?.trim();
    let event_type = required_attr(event, "type")?.trim();
    if uid.is_empty() {
        return Err(CotParseError::MissingAttribute("uid"));
    }
    if event_type.is_empty() {
        return Err(CotParseError::MissingAttribute("type"));
    }

    let point = event
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "point")
        .map(parse_point)
        .transpose()?;

    let callsign = event
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name() == "contact")
        .and_then(|node| node.attribute("callsign"))
        .map(str::to_string);

    Ok(CotEvent {
        uid: uid.to_string(),
        event_type: event_type.to_string(),
        how: event.attribute("how").map(str::to_string),
        callsign,
        time: event.attribute("time").map(str::to_string),
        start: event.attribute("start").map(str::to_string),
        stale: event.attribute("stale").map(str::to_string),
        point,
    })
}

fn parse_point(point: roxmltree::Node<'_, '_>) -> Result<CotPoint, CotParseError> {
    let lat = parse_number(point, "lat")?;
    let lon = parse_number(point, "lon")?;
    validate_coordinate("lat", lat, -90.0, 90.0)?;
    validate_coordinate("lon", lon, -180.0, 180.0)?;

    Ok(CotPoint {
        lat,
        lon,
        hae: parse_optional_number(point, "hae")?,
        ce: parse_optional_number(point, "ce")?,
        le: parse_optional_number(point, "le")?,
    })
}

fn required_attr<'a>(
    node: roxmltree::Node<'a, 'a>,
    name: &'static str,
) -> Result<&'a str, CotParseError> {
    node.attribute(name)
        .ok_or(CotParseError::MissingAttribute(name))
}

fn parse_number(node: roxmltree::Node<'_, '_>, name: &'static str) -> Result<f64, CotParseError> {
    let value = required_attr(node, name)?;
    parse_finite(value, name)
}

fn parse_optional_number(
    node: roxmltree::Node<'_, '_>,
    name: &'static str,
) -> Result<Option<f64>, CotParseError> {
    node.attribute(name)
        .map(|value| parse_finite(value, name))
        .transpose()
}

fn parse_finite(value: &str, name: &'static str) -> Result<f64, CotParseError> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| CotParseError::InvalidNumber(name))?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(CotParseError::InvalidNumber(name))
    }
}

fn validate_coordinate(
    name: &'static str,
    value: f64,
    min: f64,
    max: f64,
) -> Result<(), CotParseError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(CotParseError::InvalidCoordinate(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_COT: &[u8] = br#"
        <event version="2.0" uid="ANDROID-123" type="a-f-G-U-C"
            time="2026-06-03T10:00:00Z" start="2026-06-03T10:00:00Z"
            stale="2026-06-03T10:05:00Z" how="m-g">
            <point lat="38.8977" lon="-77.0365" hae="10.0" ce="5.0" le="5.0"/>
            <detail><contact callsign="PHONE-A"/></detail>
        </event>
    "#;

    #[test]
    fn parses_valid_cot_location() {
        let event = parse_cot_payload(VALID_COT).expect("valid CoT");
        let point = event.point.expect("point");
        assert_eq!(event.uid, "ANDROID-123");
        assert_eq!(event.event_type, "a-f-G-U-C");
        assert_eq!(event.how.as_deref(), Some("m-g"));
        assert_eq!(event.callsign.as_deref(), Some("PHONE-A"));
        assert_eq!(point.lat, 38.8977);
        assert_eq!(point.lon, -77.0365);
        assert_eq!(point.hae, Some(10.0));
    }

    #[test]
    fn validates_non_location_cot_for_forwarding() {
        let event = parse_cot_payload(br#"<event uid="chat-1" type="b-t-f"><detail/></event>"#)
            .expect("valid non-location CoT");
        assert!(event.point.is_none());
    }

    #[test]
    fn supports_location_missing_optional_precision_values() {
        let event = parse_cot_payload(
            br#"<event uid="minimal" type="a-f-G"><point lat="38" lon="-77"/></event>"#,
        )
        .expect("minimal location");
        assert!(event.point.expect("point").hae.is_none());
    }

    #[test]
    fn rejects_non_xml_payload() {
        assert!(matches!(
            parse_cot_payload(b"not a packet"),
            Err(CotParseError::Xml(_))
        ));
    }

    #[test]
    fn rejects_out_of_range_latitude() {
        let payload = br#"<event uid="bad" type="a-f-G"><point lat="91" lon="1"/></event>"#;
        assert_eq!(
            parse_cot_payload(payload).unwrap_err(),
            CotParseError::InvalidCoordinate("lat")
        );
    }

    #[test]
    fn keeps_local_and_remote_locations_separate() {
        let state = LocationState::default();
        let event = parse_cot_payload(VALID_COT).expect("valid CoT");
        state.record(PacketSource::LocalUdp, 6969, &event);
        state.record(PacketSource::RemoteReticulum, 6969, &event);
        assert!(state.get(PacketSource::LocalUdp, "ANDROID-123").is_some());
        assert!(state
            .get(PacketSource::RemoteReticulum, "ANDROID-123")
            .is_some());
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn bounds_location_records() {
        let state = LocationState::new(1, Duration::from_secs(60));
        let first =
            parse_cot_payload(br#"<event uid="one" type="a-f-G"><point lat="1" lon="1"/></event>"#)
                .unwrap();
        let second =
            parse_cot_payload(br#"<event uid="two" type="a-f-G"><point lat="2" lon="2"/></event>"#)
                .unwrap();
        state.record(PacketSource::LocalUdp, 6969, &first);
        state.record(PacketSource::LocalUdp, 6969, &second);
        assert_eq!(state.len(), 1);
    }
}
