//! PropertyMap — the key-value store on nodes and relationships.

use std::collections::HashMap;
use super::Value;

/// A map of property names to values.
pub type PropertyMap = HashMap<String, Value>;

/// Convert iterator of (key, value) pairs into a PropertyMap.
impl<K, V> From<Vec<(K, V)>> for Value
where
    K: Into<String>,
    V: Into<Value>,
{
    fn from(pairs: Vec<(K, V)>) -> Self {
        Value::Map(pairs.into_iter().map(|(k, v)| (k.into(), v.into())).collect())
    }
}
