//! WebAssembly bindings for the native Rust reader. Files stay in the browser.
use libaaf::{
    Composition, Document, ObjectId, ReaderOptions,
    defs::*,
    media::{self, ExtractFormat},
    timeline::{Clip, Curve, Item, Rational},
};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
fn js_error(e: impl std::fmt::Display) -> JsValue {
    js_sys::Error::new(&e.to_string()).into()
}
fn extract_format(format: &str) -> Result<ExtractFormat, JsValue> {
    match format {
        "original" => Ok(ExtractFormat::Original),
        "wav" => Ok(ExtractFormat::Wave),
        "bwav" => Ok(ExtractFormat::BroadcastWave),
        _ => Err(js_error("format must be original, wav, or bwav")),
    }
}
fn validate_numbers(value: &Value) -> Result<(), &'static str> {
    const MAX: u64 = 9_007_199_254_740_991;
    match value {
        Value::Number(n)
            if n.as_i64().is_some_and(|v| v.unsigned_abs() > MAX)
                || n.as_u64().is_some_and(|v| v > MAX) =>
        {
            Err("inspection integer exceeds JavaScript's safe integer range")
        }
        Value::Array(values) => values.iter().try_for_each(validate_numbers),
        Value::Object(values) => values.values().try_for_each(validate_numbers),
        _ => Ok(()),
    }
}
fn rational(r: Rational) -> Value {
    json!({"numerator":r.numerator,"denominator":r.denominator,"value":r.as_f64()})
}
fn curve(c: &Option<Curve>) -> Value {
    c.as_ref().map(|c|json!({"constant":c.constant,"interpolation":format!("{:?}",c.interpolation),"points":c.points.iter().map(|(t,v)|json!({"time":t.as_f64(),"value":v.as_f64(),"timeRational":rational(*t),"valueRational":rational(*v)})).collect::<Vec<_>>()})).unwrap_or(Value::Null)
}
fn clip(c: &Clip) -> Value {
    json!({"kind":"clip","objectId":c.object.0,"position":c.position,"length":c.length,"sourceOffset":c.source_offset,"channels":c.channels,"mute":c.mute,"name":c.subclip_name,"gain":curve(&c.gain),"automation":curve(&c.automation),"metadata":c.metadata,"sources":c.sources.iter().map(|s|json!({"essence":s.essence,"channel":s.channel})).collect::<Vec<_>>()})
}
/// An owned decoded AAF document. Call free() when replacing it.
#[wasm_bindgen]
pub struct AafDocument {
    document: Document,
    composition: Composition,
}
#[wasm_bindgen]
impl AafDocument {
    /// Parse a Uint8Array. Optional ProTools recovery matches native ReaderOptions.
    #[wasm_bindgen(constructor)]
    pub fn new(
        bytes: &[u8],
        recover_fades: bool,
        remove_sample_edits: bool,
    ) -> Result<AafDocument, JsValue> {
        let options = ReaderOptions {
            recover_protools_fades: recover_fades,
            remove_sample_accurate_edits: remove_sample_edits,
            ..ReaderOptions::default()
        };
        let document = Document::from_bytes(bytes.to_vec(), options).map_err(js_error)?;
        let composition = document.composition().map_err(js_error)?;
        Ok(Self {
            document,
            composition,
        })
    }
    /// Serializable inspection and timeline data; integers are JSON numbers.
    /// Integer fields outside JavaScript's safe integer range are rejected.
    pub fn inspect(&self) -> Result<String, JsValue> {
        let c = &self.composition;
        let d = &self.document;
        let producer=d.identification.map(|id|{let o=d.object(id);json!({"company":o.string(PID_Identification_CompanyName),"product":o.string(PID_Identification_ProductName),"version":o.string(PID_Identification_ProductVersionString)})});
        let value = json!({"name":c.name,"producer":producer,"objectCount":d.objects().len(),"classCount":d.classes.len(),"sampleRate":c.sample_rate,"sampleSize":c.sample_size,"length":c.length,"lengthRate":c.length_rate.map(rational),"metadata":c.metadata,
            "timecode":{"start":c.timecode.start,"fps":c.timecode.fps,"drop":c.timecode.drop,"editRate":rational(c.timecode.edit_rate)},
            "tracks":c.tracks.iter().map(|t|json!({"number":t.number,"name":t.name,"video":t.video,"channels":t.channels,"editRate":rational(t.edit_rate),"length":t.length,"solo":t.solo,"mute":t.mute,"gain":curve(&t.gain),"pan":curve(&t.pan),"items":t.items.iter().map(|i|match i{Item::Clip(c)=>clip(c),Item::Transition(t)=>json!({"kind":"transition","length":t.length,"cutPoint":t.cut_point,"fade":format!("{:?}",t.kind),"interpolation":format!("{:?}",t.interpolation),"curve":curve(&Some(t.level_curve()))})}).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "essences":c.essences.iter().enumerate().map(|(i,e)|json!({"id":i,"name":e.name,"uniqueName":e.unique_name,"format":format!("{:?}",e.format),"channels":e.channels,"sampleRate":e.sample_rate,"sampleSize":e.sample_size,"length":e.length,"embedded":e.embedded_node.is_some(),"originalPath":e.original_path,"metadata":e.metadata,"sourceMob":e.source_mob.0,"sourceSlot":e.source_slot,"masterMob":e.master_mob.0,"origin":e.origin,"editRate":rational(e.edit_rate)})).collect::<Vec<_>>(),
            "markers":c.markers.iter().map(|m|json!({"position":m.position,"length":m.length,"editRate":rational(m.edit_rate),"name":m.name,"comment":m.comment,"color":m.color})).collect::<Vec<_>>(),
            "diagnostics":d.diagnostics.iter().chain(&c.diagnostics).map(|d|json!({"objectId":d.object.map(|id|id.0),"message":d.message})).collect::<Vec<_>>()});
        validate_numbers(&value).map_err(js_error)?;
        serde_json::to_string(&value).map_err(js_error)
    }
    pub fn objects(&self) -> Result<String, JsValue> {
        serde_json::to_string(&self.document.objects().map(|o|json!({"id":o.id.0,"class":self.document.class_name(o.class),"path":self.document.cfb.path(o.node),"properties":o.properties().len()})).collect::<Vec<_>>()).map_err(js_error)
    }
    pub fn object(&self, id: u32) -> Result<String, JsValue> {
        let d = &self.document;
        let o = d
            .get_object(ObjectId(id))
            .ok_or_else(|| js_error("invalid object ID"))?;
        serde_json::to_string(&json!({"id":id,"class":d.class_name(o.class),"properties":o.properties().map(|p|json!({"id":p.id,"name":d.property_name(p.id),"storedForm":p.stored_form,"bytes":o.raw(p.id),"references":p.references.iter().map(|id|id.0).collect::<Vec<_>>()})).collect::<Vec<_>>() })).map_err(js_error)
    }
    pub fn extract_essence(&self, index: u32, format: &str) -> Result<Vec<u8>, JsValue> {
        let format = extract_format(format)?;
        let mut output = Vec::new();
        media::extract_essence(
            &self.document,
            &self.composition,
            index as usize,
            format,
            None,
            &mut output,
        )
        .map_err(js_error)?;
        Ok(output)
    }
    /// Extract an exact frame range. JavaScript passes offset/length as bigint.
    pub fn extract_range(
        &self,
        index: u32,
        format: &str,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, JsValue> {
        let format = extract_format(format)?;
        let mut output = Vec::new();
        media::extract_essence(
            &self.document,
            &self.composition,
            index as usize,
            format,
            Some(media::FrameRange { offset, length }),
            &mut output,
        )
        .map_err(js_error)?;
        Ok(output)
    }
    /// Extract one source of a clip, including clips composed from multiple files.
    pub fn extract_clip_source(
        &self,
        track_index: u32,
        item_index: u32,
        source_index: u32,
        format: &str,
    ) -> Result<Vec<u8>, JsValue> {
        let format = extract_format(format)?;
        let track = self
            .composition
            .tracks
            .get(track_index as usize)
            .ok_or_else(|| js_error("invalid track index"))?;
        let clip = track
            .items
            .get(item_index as usize)
            .and_then(Item::clip)
            .ok_or_else(|| js_error("invalid clip index"))?;
        let source = clip
            .sources
            .get(source_index as usize)
            .ok_or_else(|| js_error("invalid source index"))?;
        let essence = &self.composition.essences[source.essence];
        let range = media::clip_range(clip, track, essence).map_err(js_error)?;
        let mut output = Vec::new();
        media::extract_essence(
            &self.document,
            &self.composition,
            source.essence,
            format,
            Some(range),
            &mut output,
        )
        .map_err(js_error)?;
        Ok(output)
    }
    pub fn extract_clip(&self, track_index: u32, item_index: u32) -> Result<Vec<u8>, JsValue> {
        let t = self
            .composition
            .tracks
            .get(track_index as usize)
            .ok_or_else(|| js_error("invalid track index"))?;
        let clip = t
            .items
            .get(item_index as usize)
            .and_then(Item::clip)
            .ok_or_else(|| js_error("invalid clip index"))?;
        if clip.sources.len() > 1 {
            return Err(js_error("clip extraction requires a single media source"));
        }
        let s = clip
            .sources
            .first()
            .ok_or_else(|| js_error("clip has no source"))?;
        let e = &self.composition.essences[s.essence];
        let range = media::clip_range(clip, t, e).map_err(js_error)?;
        let mut out = Vec::new();
        media::extract_essence(
            &self.document,
            &self.composition,
            s.essence,
            ExtractFormat::Wave,
            Some(range),
            &mut out,
        )
        .map_err(js_error)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::validate_numbers;
    use serde_json::json;

    #[test]
    fn reject_lossy_nested_integer_fields() {
        assert!(
            validate_numbers(&json!({"tracks": [{"length": 9_007_199_254_740_991_i64}]})).is_ok()
        );
        for number in [
            json!(9_007_199_254_740_992_u64),
            json!(-9_007_199_254_740_992_i64),
            json!(u64::MAX),
        ] {
            assert!(validate_numbers(&json!({"essences": [{"length": number}]})).is_err());
        }
    }
}
