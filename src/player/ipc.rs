//! mpv's JSON IPC protocol: one JSON object per line in each direction.
//! See <https://mpv.io/manual/stable/#json-ipc>.

use serde_json::{Value, json};

/// Property observer ids used with `observe_property`.
pub const OBSERVE_TIME_POS: u64 = 1;
pub const OBSERVE_DURATION: u64 = 2;
pub const OBSERVE_PAUSE: u64 = 3;
pub const OBSERVE_LEVELS: u64 = 4;

/// A loudness meter for the visualizer: FFmpeg's `astats` passes audio
/// through untouched and reports overall RMS and peak (dBFS) per frame as
/// metadata, which mpv exposes as the `af-metadata/<label>` property.
pub const METER_LABEL: &str = "@shellify-meter";
pub const METER_FILTER: &str = "@shellify-meter:lavfi=[astats=metadata=1:reset=1:measure_overall=RMS_level+Peak_level:measure_perchannel=none]";
pub const METER_PROPERTY: &str = "af-metadata/shellify-meter";

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    LoadFile(String),
    SetPause(bool),
    SeekAbsolute(f64),
    SetVolume(u8),
    Stop,
    Quit,
    Observe(u64, &'static str),
    AddAudioFilter(&'static str),
    RemoveAudioFilter(&'static str),
}

impl Command {
    /// Encodes the command as a newline-terminated JSON line.
    pub fn encode(&self, request_id: u64) -> String {
        let args = match self {
            Self::LoadFile(source) => json!(["loadfile", source, "replace"]),
            Self::SetPause(p) => json!(["set_property", "pause", p]),
            Self::SeekAbsolute(secs) => json!(["seek", secs, "absolute"]),
            Self::SetVolume(v) => json!(["set_property", "volume", v]),
            Self::Stop => json!(["stop"]),
            Self::Quit => json!(["quit"]),
            Self::Observe(id, name) => json!(["observe_property", id, name]),
            Self::AddAudioFilter(spec) => json!(["af", "add", spec]),
            Self::RemoveAudioFilter(label) => json!(["af", "remove", label]),
        };
        let mut line = json!({ "command": args, "request_id": request_id }).to_string();
        line.push('\n');
        line
    }
}

/// A message received from mpv.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Reply to a command we sent.
    Response {
        request_id: u64,
        /// `None` on success, otherwise mpv's error string.
        error: Option<String>,
        data: Value,
    },
    StartFile {
        entry: u64,
    },
    EndFile {
        entry: u64,
        reason: String,
        /// Set for `reason == "error"`.
        file_error: Option<String>,
        /// For `reason == "redirect"`: the first playlist entry the source
        /// expanded into, and how many entries were inserted.
        insert_id: Option<u64>,
        insert_count: u64,
    },
    FileLoaded,
    PropertyChange {
        id: u64,
        /// `Null` when the property is unavailable (e.g. nothing playing).
        data: Value,
    },
    /// An event we don't use.
    Other,
}

impl Incoming {
    pub fn parse(line: &str) -> Result<Self, serde_json::Error> {
        let v: Value = serde_json::from_str(line)?;
        let u64_field = |name: &str| v.get(name).and_then(Value::as_u64);
        let str_field = |name: &str| v.get(name).and_then(Value::as_str).map(str::to_owned);

        let Some(event) = v.get("event").and_then(Value::as_str) else {
            let error = str_field("error").filter(|e| e != "success");
            return Ok(Self::Response {
                request_id: u64_field("request_id").unwrap_or(0),
                error,
                data: v.get("data").cloned().unwrap_or(Value::Null),
            });
        };

        Ok(match event {
            "start-file" => Self::StartFile {
                entry: u64_field("playlist_entry_id").unwrap_or(0),
            },
            "end-file" => Self::EndFile {
                entry: u64_field("playlist_entry_id").unwrap_or(0),
                reason: str_field("reason").unwrap_or_default(),
                file_error: str_field("file_error"),
                insert_id: u64_field("playlist_insert_id"),
                insert_count: u64_field("playlist_insert_num_entries").unwrap_or(0),
            },
            "file-loaded" => Self::FileLoaded,
            "property-change" => Self::PropertyChange {
                id: u64_field("id").unwrap_or(0),
                data: v.get("data").cloned().unwrap_or(Value::Null),
            },
            _ => Self::Other,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn encodes_meter_filter_commands() {
        let add: serde_json::Value =
            serde_json::from_str(&Command::AddAudioFilter(METER_FILTER).encode(7)).unwrap();
        assert_eq!(add["command"][0], "af");
        assert_eq!(add["command"][1], "add");
        assert!(
            add["command"][2]
                .as_str()
                .unwrap()
                .starts_with("@shellify-meter:lavfi=[astats")
        );
        let remove: serde_json::Value =
            serde_json::from_str(&Command::RemoveAudioFilter(METER_LABEL).encode(8)).unwrap();
        assert_eq!(
            remove["command"],
            json!(["af", "remove", "@shellify-meter"])
        );
        assert_eq!(METER_PROPERTY, format!("af-metadata/{}", &METER_LABEL[1..]));
    }

    use super::*;

    fn decode(line: &str) -> Value {
        serde_json::from_str(line.trim_end()).unwrap()
    }

    #[test]
    fn encodes_commands_as_json_lines() {
        let line = Command::LoadFile("ytdl://ytsearch1:a b".into()).encode(7);
        assert!(line.ends_with('\n'));
        assert_eq!(
            decode(&line),
            json!({"command": ["loadfile", "ytdl://ytsearch1:a b", "replace"], "request_id": 7})
        );
        assert_eq!(
            decode(&Command::SeekAbsolute(90.0).encode(0))["command"],
            json!(["seek", 90.0, "absolute"])
        );
        assert_eq!(
            decode(&Command::SetVolume(55).encode(0))["command"],
            json!(["set_property", "volume", 55])
        );
        assert_eq!(
            decode(&Command::Observe(OBSERVE_PAUSE, "pause").encode(0))["command"],
            json!(["observe_property", 3, "pause"])
        );
    }

    #[test]
    fn parses_responses() {
        assert_eq!(
            Incoming::parse(r#"{"data":{"playlist_entry_id":2},"request_id":7,"error":"success"}"#)
                .unwrap(),
            Incoming::Response {
                request_id: 7,
                error: None,
                data: json!({"playlist_entry_id": 2}),
            }
        );
        assert_eq!(
            Incoming::parse(r#"{"request_id":0,"error":"property unavailable"}"#).unwrap(),
            Incoming::Response {
                request_id: 0,
                error: Some("property unavailable".into()),
                data: Value::Null,
            }
        );
    }

    #[test]
    fn parses_file_events() {
        assert_eq!(
            Incoming::parse(r#"{"event":"start-file","playlist_entry_id":3}"#).unwrap(),
            Incoming::StartFile { entry: 3 }
        );
        assert_eq!(
            Incoming::parse(
                r#"{"event":"end-file","reason":"error","playlist_entry_id":3,"file_error":"loading failed"}"#
            )
            .unwrap(),
            Incoming::EndFile {
                entry: 3,
                reason: "error".into(),
                file_error: Some("loading failed".into()),
                insert_id: None,
                insert_count: 0,
            }
        );
        assert_eq!(
            Incoming::parse(
                r#"{"event":"end-file","reason":"redirect","playlist_entry_id":1,"playlist_insert_id":2,"playlist_insert_num_entries":1}"#
            )
            .unwrap(),
            Incoming::EndFile {
                entry: 1,
                reason: "redirect".into(),
                file_error: None,
                insert_id: Some(2),
                insert_count: 1,
            }
        );
        assert_eq!(
            Incoming::parse(r#"{"event":"file-loaded"}"#).unwrap(),
            Incoming::FileLoaded
        );
        assert_eq!(
            Incoming::parse(r#"{"event":"audio-reconfig"}"#).unwrap(),
            Incoming::Other
        );
    }

    #[test]
    fn parses_property_changes_including_unavailable() {
        assert_eq!(
            Incoming::parse(r#"{"event":"property-change","id":1,"name":"time-pos","data":12.5}"#)
                .unwrap(),
            Incoming::PropertyChange {
                id: 1,
                data: json!(12.5)
            }
        );
        assert_eq!(
            Incoming::parse(r#"{"event":"property-change","id":1,"name":"time-pos"}"#).unwrap(),
            Incoming::PropertyChange {
                id: 1,
                data: Value::Null
            }
        );
    }

    #[test]
    fn rejects_malformed_lines() {
        assert!(Incoming::parse("not json").is_err());
    }
}
