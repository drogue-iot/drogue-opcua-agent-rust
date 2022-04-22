use chrono::{SecondsFormat, Utc};
use humantime::format_rfc3339_millis;
use opcua::types::{DataValue, DateTime, QualifiedName, StatusCode, UAString, Variant};
use serde_json::{json, Map, Value};
use std::time::SystemTime;

pub trait ToJson {
    fn to_json(self) -> Value;
}

impl ToJson for SystemTime {
    fn to_json(self) -> Value {
        format_rfc3339_millis(self).to_string().into()
    }
}

impl ToJson for chrono::DateTime<Utc> {
    fn to_json(self) -> Value {
        self.to_rfc3339_opts(SecondsFormat::Millis, true).into()
    }
}

impl ToJson for DataValue {
    fn to_json(self) -> Value {
        let mut m = Map::<String, Value>::new();

        // get any timestamp

        let timestamp = self
            .source_timestamp
            .or_else(|| self.server_timestamp)
            .map(|ts| ts.as_chrono())
            .unwrap_or_else(|| Utc::now());
        m.insert("timestamp".to_string(), timestamp.to_json());

        if let Some(source_timestamp) = self.source_timestamp {
            m.insert("source_timestamp".to_string(), source_timestamp.to_json());
        }
        if let Some(server_timestamp) = self.server_timestamp {
            m.insert("server_timestamp".to_string(), server_timestamp.to_json());
        }

        m.insert("value".to_string(), self.value.to_json());
        m.insert(
            "status".to_string(),
            self.status.unwrap_or(StatusCode::Good).to_json(),
        );

        Value::Object(m)
    }
}

impl ToJson for DateTime {
    fn to_json(self) -> Value {
        self.as_chrono().to_json()
    }
}

impl ToJson for StatusCode {
    fn to_json(self) -> Value {
        self.name().into()
    }
}

impl ToJson for Vec<u8> {
    fn to_json(self) -> Value {
        base64::encode(&self).into()
    }
}

impl ToJson for UAString {
    fn to_json(self) -> Value {
        self.as_ref().into()
    }
}

impl ToJson for QualifiedName {
    fn to_json(self) -> Value {
        json!({
            "namespace": self.namespace_index,
            "name": self.name.to_json(),
        })
    }
}

impl ToJson for Variant {
    fn to_json(self) -> Value {
        match self {
            Variant::Empty => Value::Null,
            Variant::Boolean(value) => value.into(),
            Variant::SByte(value) => value.into(),
            Variant::Byte(value) => value.into(),
            Variant::Int16(value) => value.into(),
            Variant::UInt16(value) => value.into(),
            Variant::Int32(value) => value.into(),
            Variant::UInt32(value) => value.into(),
            Variant::Int64(value) => value.into(),
            Variant::UInt64(value) => value.into(),
            Variant::Float(value) => value.into(),
            Variant::Double(value) => value.into(),
            Variant::String(value) => value.to_json(),
            Variant::DateTime(value) => value.to_json(),
            Variant::Guid(value) => value.to_string().into(),
            Variant::StatusCode(value) => value.to_json(),
            Variant::ByteString(value) => value.value.to_json(),
            Variant::XmlElement(value) => value.to_json(),
            Variant::QualifiedName(value) => value.to_json(),
            Variant::LocalizedText(value) => value.to_string().into(),
            Variant::NodeId(value) => value.to_string().into(),
            Variant::ExpandedNodeId(value) => value.to_string().into(),
            Variant::ExtensionObject(value) => serde_json::to_value(&value).unwrap_or_default(),
            Variant::Variant(value) => value.to_json(),
            Variant::DataValue(value) => value.to_json(),
            Variant::Diagnostics(value) => serde_json::to_value(&value).unwrap_or_default(),
            Variant::Array(values) => values.values.into_iter().map(|v| v.to_json()).collect(),
        }
    }
}

impl<T> ToJson for Option<T>
where
    T: ToJson,
{
    fn to_json(self) -> Value {
        match self {
            None => Value::Null,
            Some(value) => value.to_json(),
        }
    }
}
