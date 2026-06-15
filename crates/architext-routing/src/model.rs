//! Plan/Input data model mirroring the JS planCodec wire shape exactly. Maps are
//! IndexMap because the router's tiebreaks depend on JS insertion order. The Map
//! fields arrive as `[key, value]` entries arrays, not JSON objects. One Set field
//! arrives as a plain JSON array.
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// entries_map: serde adapter for IndexMap<String, V> <-> [[key, value], ...]
// The JS codec emits Map fields as Array.from(map.entries()) — a JSON array of
// 2-element arrays. serde's default IndexMap deserialization expects a JSON
// object, so we need this adapter for every Map field.
// ---------------------------------------------------------------------------
mod entries_map {
    use indexmap::IndexMap;
    use serde::de::{DeserializeOwned, Deserializer, SeqAccess, Visitor};
    use serde::ser::{Serialize, SerializeSeq, Serializer};
    use std::fmt;
    use std::marker::PhantomData;

    pub fn serialize<V, S>(map: &IndexMap<String, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        V: Serialize,
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(map.len()))?;
        for (k, v) in map {
            seq.serialize_element(&(k, v))?;
        }
        seq.end()
    }

    pub fn deserialize<'de, V, D>(deserializer: D) -> Result<IndexMap<String, V>, D::Error>
    where
        V: DeserializeOwned,
        D: Deserializer<'de>,
    {
        struct EntriesVisitor<V>(PhantomData<V>);

        impl<'de, V: DeserializeOwned> Visitor<'de> for EntriesVisitor<V> {
            type Value = IndexMap<String, V>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an array of [key, value] pairs")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut map = IndexMap::new();
                while let Some((k, v)) = seq.next_element::<(String, V)>()? {
                    map.insert(k, v);
                }
                Ok(map)
            }
        }

        deserializer.deserialize_seq(EntriesVisitor(PhantomData))
    }
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A routed edge. The JS router attaches many diagnostic fields alongside the
/// core geometry; we capture them in `extra` so the model round-trips without
/// loss even as the router evolves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub d: String,
    pub points: Vec<Point>,
    #[serde(rename = "labelX")]
    pub label_x: f64,
    #[serde(rename = "labelY")]
    pub label_y: f64,
    #[serde(flatten)]
    pub extra: IndexMap<String, serde_json::Value>,
}

/// The full plan wire shape produced by `serializePlan` in planCodec.js.
/// Map fields arrive as `[[key, value], ...]` entries arrays; `visibleNodeIds`
/// arrives as a plain JSON array (serialized from a JS Set).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub node_width: f64,
    pub node_height: f64,
    pub lane_width: f64,
    pub row_gap: f64,
    pub margin_x: f64,
    pub margin_y: f64,
    /// Serialized from a JS Set — arrives as a plain JSON array.
    pub visible_node_ids: IndexSet<String>,
    /// Map<nodeId, laneIndex> — wire shape is `[[key, value], ...]`.
    #[serde(with = "entries_map")]
    pub lane_index_by_node: IndexMap<String, i64>,
    /// Map<nodeId, rowIndex> — wire shape is `[[key, value], ...]`.
    #[serde(with = "entries_map")]
    pub row_index_by_node: IndexMap<String, i64>,
    /// Map<nodeId, Rect> — wire shape is `[[key, value], ...]`.
    #[serde(with = "entries_map")]
    pub node_rects: IndexMap<String, Rect>,
    /// Map<edgeId, Route> — wire shape is `[[key, value], ...]`.
    #[serde(with = "entries_map")]
    pub routes: IndexMap<String, Route>,
    /// Map<edgeId, Rect> — wire shape is `[[key, value], ...]`.
    #[serde(with = "entries_map")]
    pub label_boxes: IndexMap<String, Rect>,
    #[serde(default)]
    pub warnings: Vec<serde_json::Value>,
}
