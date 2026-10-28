use super::*;
use crate::application::workflow::jev::JevRequest;
use image::ImageEncoder;
use serde_json::json;

pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &vec![127; (width * height) as usize],
            width,
            height,
            image::ExtendedColorType::L8,
        )
        .unwrap();
    bytes
}
pub(crate) fn inputs(bytes: &[u8]) -> Value {
    json!({"connection":"service","model":"jev-1.14.0","state":{"note":"photo category stays text"},"assets":{"photo":{"channel_id":uuid::Uuid::from_u128(1),"thread_id":uuid::Uuid::from_u128(2),"sha256":ImageDigest::of(bytes)}},"questions":{"visible":{"type":"noul","instructions":"Is there a mug in `assets.photo`?"},"mug":{"type":"box","instructions":"The mug in `assets.photo`"}}})
}
pub(crate) fn output() -> Value {
    json!({"model":"jev-1.14.0","answers":{"visible":{"type":"noul","noul":0.9},"mug":{"type":"box","box":{"left":0.1,"right":0.8,"top":0.2,"bottom":0.9},"confidence":0.8}},"usage":{"input_tokens":200,"output_tokens":20}})
}
pub(crate) fn hydrated(inputs: &Value, bytes: Vec<u8>) -> JevRequest {
    let refs = parse_assets(inputs.get("assets")).unwrap();
    let (name, reference) = refs.first_key_value().unwrap();
    JevRequest::prepare(inputs)
        .unwrap()
        .hydrate(BTreeMap::from([(
            name.clone(),
            JevImage::validate(reference, "image/png", bytes).unwrap(),
        )]))
        .unwrap()
}
#[test]
fn jev_assets_identity_opaque_wire_and_pixel_reservation() {
    let bytes = png(280, 280);
    let inputs = inputs(&bytes);
    let request = hydrated(&inputs, bytes.clone());
    let second = hydrated(&inputs, bytes);
    assert_eq!(request.body(), second.body());
    assert_eq!(request.digest(), second.digest());
    assert!(request.ready() && request.has_images());
    let wire: Value = serde_json::from_slice(request.body()).unwrap();
    let identity: Value = serde_json::from_slice(request.identity()).unwrap();
    let token = wire["state"]["assets"]["photo"].as_str().unwrap();
    assert_eq!(token.len(), 74);
    assert_eq!(wire["state"]["content"], inputs["state"]);
    assert!(wire["assets"][token]["data_base64"].is_string());
    assert_eq!(identity["assets"], inputs["assets"]);
    assert!(!String::from_utf8_lossy(request.identity()).contains("data_base64"));
    assert_eq!(
        request.reserved_input(),
        request.body().len() as i64 + 280 * 280
    );
    assert!(!JevRequest::prepare(&inputs).unwrap().ready());
}
#[test]
fn jev_assets_reject_changed_unsupported_oversized_and_invalid_dimensions() {
    let bytes = png(280, 280);
    let refs = parse_assets(inputs(&bytes).get("assets")).unwrap();
    let reference = refs.values().next().unwrap();
    assert!(matches!(
        JevImage::validate(reference, "image/png", png(281, 280)),
        Err(JevAssetError::Changed)
    ));
    assert!(matches!(
        JevImage::validate(reference, "text/plain", bytes),
        Err(JevAssetError::Unsupported)
    ));
    assert!(matches!(
        JevImage::validate(reference, "image/png", vec![0; MAX_IMAGE_BYTES + 1]),
        Err(JevAssetError::TooLarge)
    ));
    for bytes in [png(279, 280), png(1025, 280), b"broken image".to_vec()] {
        let mut reference = reference.clone();
        reference.sha256 = ImageDigest::of(&bytes);
        assert!(matches!(
            JevImage::validate(&reference, "image/png", bytes),
            Err(JevAssetError::Unsupported)
        ));
    }
}
#[test]
fn jev_assets_forbid_inline_url_mutable_and_excess_references() {
    let bytes = png(280, 280);
    let valid = inputs(&bytes)["assets"].clone();
    for value in [
        json!({}),
        json!({"photo":"https://example.test/image.png"}),
        json!({"photo":{"url":"https://example.test"}}),
        json!({"photo":{"data_base64":"AAAA"}}),
    ] {
        assert!(parse_assets(Some(&value)).is_err());
    }
    for field in ["sha256", "channel_id", "thread_id"] {
        let mut bad = valid.clone();
        bad["photo"].as_object_mut().unwrap().remove(field);
        assert!(parse_assets(Some(&bad)).is_err());
    }
    for count in [4, 5] {
        let map: serde_json::Map<_, _> = (0..count)
            .map(|n| (format!("p{n}"), valid["photo"].clone()))
            .collect();
        assert_eq!(parse_assets(Some(&json!(map))).is_ok(), count == 4);
    }
}
#[test]
fn jev_assets_aggregate_wire_bound_is_still_64_kib() {
    let bytes = png(280, 280);
    let mut inputs = inputs(&bytes);
    inputs["state"] = json!("a".repeat(64_000));
    let refs = parse_assets(inputs.get("assets")).unwrap();
    let (name, reference) = refs.first_key_value().unwrap();
    let request = JevRequest::prepare(&inputs).unwrap();
    assert!(
        request
            .hydrate(BTreeMap::from([(
                name.clone(),
                JevImage::validate(reference, "image/png", bytes).unwrap()
            )]))
            .is_err()
    );
}

#[test]
fn jev_assets_jpeg_and_animation_are_explicitly_validated() {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .write_image(
            &vec![127; 280 * 280],
            280,
            280,
            image::ExtendedColorType::L8,
        )
        .unwrap();
    let refs = parse_assets(inputs(&jpeg).get("assets")).unwrap();
    let reference = refs.values().next().unwrap();
    assert_eq!(
        JevImage::validate(reference, "image/jpeg", jpeg.clone())
            .unwrap()
            .format(),
        "jpeg"
    );
    assert!(matches!(
        JevImage::validate(reference, "image/png", jpeg),
        Err(JevAssetError::Unsupported)
    ));
    let animated = include_bytes!("testdata/jev-animated.png").to_vec();
    assert!(
        image::codecs::png::PngDecoder::new(std::io::Cursor::new(&animated))
            .unwrap()
            .is_apng()
            .unwrap()
    );
    let refs = parse_assets(inputs(&animated).get("assets")).unwrap();
    assert!(matches!(
        JevImage::validate(refs.values().next().unwrap(), "image/png", animated),
        Err(JevAssetError::Unsupported)
    ));
}
