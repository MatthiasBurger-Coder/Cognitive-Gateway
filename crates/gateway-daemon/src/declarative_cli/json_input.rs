//! JSON parsing with duplicate-key rejection before typed adapter validation.
use serde::{
    Deserialize, Deserializer,
    de::{Error as _, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Value, json};
use std::fmt;

pub(super) struct StrictValue(pub Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StrictVisitor;
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut result = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
                    if result.insert(key, value.0).is_some() {
                        return Err(M::Error::custom("duplicate object key"));
                    }
                }
                Ok(StrictValue(Value::Object(result)))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut result = vec![];
                while let Some(v) = seq.next_element::<StrictValue>()? {
                    result.push(v.0);
                }
                Ok(StrictValue(Value::Array(result)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(StrictValue(json!(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
        }
        deserializer.deserialize_any(StrictVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_non_json_bytes_instead_of_coercing_them() {
        let deserializer =
            serde::de::value::BytesDeserializer::<serde::de::value::Error>::new(b"raw");
        let error = match StrictValue::deserialize(deserializer) {
            Err(error) => error,
            Ok(_) => panic!("raw bytes must not become JSON"),
        };
        assert!(error.to_string().contains("JSON with unique object keys"));
    }
}
