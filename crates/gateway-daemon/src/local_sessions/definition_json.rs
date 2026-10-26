//! Bounded definition parsing before any current authority is reconstructed.
use super::SessionError;
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::fmt;

const MAX_BYTES: usize = 1_048_576;
const MAX_DEPTH: usize = 64;

pub(super) fn parse(text: &str) -> Result<Value, SessionError> {
    if text.len() > MAX_BYTES {
        return Err(SessionError::LimitExceeded);
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = Node(0)
        .deserialize(&mut deserializer)
        .map_err(|_| SessionError::InvalidInput)?;
    deserializer.end().map_err(|_| SessionError::InvalidInput)?;
    Ok(value)
}

struct Node(usize);
impl<'de> DeserializeSeed<'de> for Node {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> Result<Value, D::Error> {
        if self.0 > MAX_DEPTH {
            return Err(D::Error::custom("definition depth exceeded"));
        }
        de.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Node {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a bounded JSON definition with unique object keys")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> Result<Value, A::Error> {
        let mut object = Map::new();
        while let Some(key) = input.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(A::Error::custom("duplicate definition field"));
            }
            let value = input.next_value_seed(Node(self.0 + 1))?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = input.next_element_seed(Node(self.0 + 1))? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_bool<E: Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("invalid definition number"))
    }
    fn visit_str<E: Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }
    fn visit_string<E: Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }
    fn visit_unit<E: Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
}

#[cfg(test)]
#[path = "definition_json/tests.rs"]
mod tests;
