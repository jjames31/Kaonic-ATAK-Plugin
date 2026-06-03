use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq)]
pub struct CotPoint {
    pub lat: f64,
    pub lon: f64,
    pub hae: f64,
    pub ce: f64,
    pub le: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CotEvent {
    pub uid: String,
    pub event_type: String,
    pub time: Option<String>,
    pub start: Option<String>,
    pub stale: Option<String>,
    pub point: CotPoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CotParseError {
    NotUtf8,
    Xml(String),
    WrongRoot,
    MissingAttribute(&'static str),
    MissingPoint,
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
            Self::MissingPoint => write!(f, "missing CoT point"),
            Self::InvalidNumber(attr) => write!(f, "invalid CoT numeric attribute {attr}"),
            Self::InvalidCoordinate(attr) => write!(f, "invalid CoT coordinate {attr}"),
        }
    }
}

impl std::error::Error for CotParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketSource {
    LocalUdp,
    RemoteReticulum,
}

#[derive(Debug, Clone)]
pub struct LocationRecord {
    pub uid: String,
    pub event_type: String,
    pub point: CotPoint,
    pub source: PacketSource,
    pub channel_port: u16,
    pub updated_at: SystemTime,
}

#[derive(Default)]
pub struct LocationState {
    records: Mutex<HashMap<String, LocationRecord>>,
}

impl LocationState {
    pub fn record(
        &self,
        source: PacketSource,
        channel_port: u16,
        event: &CotEvent,
    ) -> LocationRecord {
        let record = LocationRecord {
            uid: event.uid.clone(),
            event_type: event.event_type.clone(),
            point: event.point.clone(),
            source,
            channel_port,
            updated_at: SystemTime::now(),
        };

        let mut records = self.records.lock().expect("location state poisoned");
        records.insert(record.uid.clone(), record);
        records.get(&event.uid).expect("record inserted").clone()
    }

    pub fn len(&self) -> usize {
        self.records.lock().expect("location state poisoned").len()
    }

    #[cfg(test)]
    pub fn get(&self, uid: &str) -> Option<LocationRecord> {
        self.records
            .lock()
            .expect("location state poisoned")
            .get(uid)
            .cloned()
    }
}

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
        .ok_or(CotParseError::MissingPoint)?;

    let lat = parse_number(point, "lat")?;
    let lon = parse_number(point, "lon")?;
    let hae = parse_number(point, "hae")?;
    let ce = parse_number(point, "ce")?;
    let le = parse_number(point, "le")?;

    validate_coordinate("lat", lat, -90.0, 90.0)?;
    validate_coordinate("lon", lon, -180.0, 180.0)?;

    Ok(CotEvent {
        uid: uid.to_string(),
        event_type: event_type.to_string(),
        time: event.attribute("time").map(str::to_string),
        start: event.attribute("start").map(str::to_string),
        stale: event.attribute("stale").map(str::to_string),
        point: CotPoint {
            lat,
            lon,
            hae,
            ce,
            le,
        },
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
            <detail/>
        </event>
    "#;

    #[test]
    fn parses_valid_cot_location() {
        let event = parse_cot_payload(VALID_COT).expect("valid CoT");

        assert_eq!(event.uid, "ANDROID-123");
        assert_eq!(event.event_type, "a-f-G-U-C");
        assert_eq!(event.point.lat, 38.8977);
        assert_eq!(event.point.lon, -77.0365);
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
        let payload = br#"
            <event uid="bad" type="a-f-G-U-C">
                <point lat="91" lon="1" hae="0" ce="1" le="1"/>
            </event>
        "#;

        assert_eq!(
            parse_cot_payload(payload).unwrap_err(),
            CotParseError::InvalidCoordinate("lat")
        );
    }

    #[test]
    fn records_latest_location_by_uid() {
        let state = LocationState::default();
        let event = parse_cot_payload(VALID_COT).expect("valid CoT");

        state.record(PacketSource::LocalUdp, 6969, &event);

        let record = state.get("ANDROID-123").expect("recorded location");
        assert_eq!(record.source, PacketSource::LocalUdp);
        assert_eq!(record.channel_port, 6969);
        assert_eq!(record.event_type, "a-f-G-U-C");
        assert_eq!(record.point.lat, 38.8977);
        assert!(record.updated_at <= SystemTime::now());
        assert_eq!(state.len(), 1);
    }
}
