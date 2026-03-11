//! Shared log types used by both agent and app for execution output.

#[derive(Debug, Clone)]
pub struct LogLine {
    pub stream: LogStream,
    pub text: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

impl serde::Serialize for LogStream {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            LogStream::Stdout => serializer.serialize_str("stdout"),
            LogStream::Stderr => serializer.serialize_str("stderr"),
        }
    }
}

impl serde::Serialize for LogLine {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("LogLine", 3)?;
        s.serialize_field("stream", &self.stream)?;
        s.serialize_field("text", &self.text)?;
        s.serialize_field("timestamp", &self.timestamp.to_rfc3339())?;
        s.end()
    }
}
