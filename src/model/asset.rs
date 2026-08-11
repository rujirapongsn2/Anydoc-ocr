/// Index into `Document::assets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize), serde(transparent))]
pub struct AssetId(pub usize);

/// An embedded binary asset (image, object payload). Bytes are always
/// retained so the document stays self-contained; total retained bytes are
/// capped by the fixed `max_asset_total_bytes` limit at parse time.
///
/// JSON carries metadata only: the payload becomes a `byteLength` count, so
/// serializing a document never inflates it with megabytes of base64.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "json", derive(serde::Serialize), serde(rename_all = "camelCase"))]
pub struct Asset {
    /// This asset's own index, so a detached `Asset` still identifies itself.
    pub id: AssetId,
    /// MIME type, e.g. `image/png`.
    pub media_type: String,
    /// Package part or stream the asset came from, for provenance.
    pub origin_part: String,
    /// The payload, exactly as stored in the source.
    #[cfg_attr(
        feature = "json",
        serde(rename = "byteLength", serialize_with = "serialize_byte_len")
    )]
    pub bytes: Vec<u8>,
}

#[cfg(feature = "json")]
fn serialize_byte_len<S: serde::Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(bytes.len() as u64)
}
