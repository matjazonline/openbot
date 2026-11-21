//! Normalized campaign/ad/link commands and immutable click identity.
use super::{
    identity::{Failure, Result},
    ids::entity_id,
    setup::{StoreId, destination, name},
};
use serde::{Deserialize, Serialize};
entity_id!(CampaignId);
entity_id!(AdId);
entity_id!(LinkId);
entity_id!(ClickId);

fn optional<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
fn nullable<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}
fn nullable_update<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CampaignWrite {
    pub store_id: StoreId,
    pub name: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionUpdate {
    #[serde(default, deserialize_with = "optional")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "optional")]
    pub enabled: Option<bool>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdWrite {
    pub store_id: StoreId,
    pub campaign_id: CampaignId,
    pub name: String,
    #[serde(deserialize_with = "nullable")]
    pub advertised_product_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdUpdate {
    #[serde(default, deserialize_with = "optional")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "optional")]
    pub enabled: Option<bool>,
    // Absence and an explicit clear are different effects and fingerprints.
    #[serde(
        default,
        deserialize_with = "nullable_update",
        skip_serializing_if = "Option::is_none"
    )]
    pub advertised_product_id: Option<Option<String>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkWrite {
    pub store_id: StoreId,
    pub ad_id: AdId,
    pub destination: String,
    pub placement: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkUpdate {
    #[serde(default, deserialize_with = "optional")]
    pub destination: Option<String>,
    #[serde(default, deserialize_with = "optional")]
    pub placement: Option<String>,
    #[serde(default, deserialize_with = "optional")]
    pub enabled: Option<bool>,
}
#[derive(Clone, Serialize)]
pub enum Mutation {
    CreateCampaign(CampaignWrite),
    UpdateCampaign(CampaignId, DimensionUpdate),
    CreateAd(AdWrite),
    UpdateAd(AdId, AdUpdate),
    CreateLink(LinkWrite),
    UpdateLink(LinkId, LinkUpdate),
}
impl Mutation {
    pub fn normalize(mut self) -> Result<Self> {
        match &mut self {
            Self::CreateCampaign(w) => name(&mut w.name, 120)?,
            Self::UpdateCampaign(_, w) => {
                if w.name.is_none() && w.enabled.is_none() {
                    return Err(Failure::Invalid);
                }
                if let Some(n) = &mut w.name {
                    name(n, 120)?;
                }
            }
            Self::CreateAd(w) => {
                name(&mut w.name, 120)?;
                product(w.advertised_product_id.as_deref())?;
            }
            Self::UpdateAd(_, w) => {
                if w.name.is_none() && w.enabled.is_none() && w.advertised_product_id.is_none() {
                    return Err(Failure::Invalid);
                }
                if let Some(n) = &mut w.name {
                    name(n, 120)?;
                }
                if let Some(p) = &w.advertised_product_id {
                    product(p.as_deref())?;
                }
            }
            Self::CreateLink(w) => {
                w.destination = link_destination(&w.destination)?;
                name(&mut w.placement, 120)?;
            }
            Self::UpdateLink(_, w) => {
                if w.destination.is_none() && w.placement.is_none() && w.enabled.is_none() {
                    return Err(Failure::Invalid);
                }
                if let Some(d) = &mut w.destination {
                    *d = link_destination(d)?;
                }
                if let Some(p) = &mut w.placement {
                    name(p, 120)?;
                }
            }
        }
        Ok(self)
    }
    pub fn operation(&self) -> String {
        match self {
            Self::CreateCampaign(_) => "POST /v1/campaigns".into(),
            Self::UpdateCampaign(id, _) => format!("PATCH /v1/campaigns/{}", id.0),
            Self::CreateAd(_) => "POST /v1/ads".into(),
            Self::UpdateAd(id, _) => format!("PATCH /v1/ads/{}", id.0),
            Self::CreateLink(_) => "POST /v1/links".into(),
            Self::UpdateLink(id, _) => format!("PATCH /v1/links/{}", id.0),
        }
    }
    pub fn action(&self) -> &'static str {
        match self {
            Self::CreateCampaign(_) => "campaign.create",
            Self::UpdateCampaign(..) => "campaign.update",
            Self::CreateAd(_) => "ad.create",
            Self::UpdateAd(..) => "ad.update",
            Self::CreateLink(_) => "link.create",
            Self::UpdateLink(..) => "link.update",
        }
    }
    pub fn supplied_store(&self) -> Option<StoreId> {
        match self {
            Self::CreateCampaign(w) => Some(w.store_id),
            Self::CreateAd(w) => Some(w.store_id),
            Self::CreateLink(w) => Some(w.store_id),
            _ => None,
        }
    }
}
pub fn link_destination(input: &str) -> Result<String> {
    let url = destination(input)?;
    if url.query_pairs().any(|(key, _)| key == "ob_click") {
        return Err(Failure::Invalid);
    }
    Ok(url.into())
}
fn product(value: Option<&str>) -> Result<()> {
    if value.is_some_and(|s| {
        s.is_empty() || s.chars().count() > 128 || s.bytes().any(|b| b <= 32 || b == 127)
    }) {
        return Err(Failure::Invalid);
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    Campaigns,
    Ads,
    Links,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Classification {
    pub is_bot: bool,
    pub is_prefetch: bool,
    pub version: i16,
}
/// Heuristic signals only. The adapter supplies at most512 bytes per header;
/// neither these headers nor any visitor address belongs in persistent evidence.
pub fn classify(user_agent: &str, purpose: &str, sec_purpose: &str) -> Classification {
    let bounded = |s: &str| s.chars().take(512).collect::<String>().to_ascii_lowercase();
    let ua = bounded(user_agent);
    Classification {
        is_bot: [
            "bot", "crawler", "spider", "headless", "preview", "curl/", "wget/",
        ]
        .iter()
        .any(|token| ua.contains(token)),
        is_prefetch: bounded(purpose).contains("prefetch")
            || bounded(sec_purpose).contains("prefetch"),
        version: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attribution_links_product_clear_has_distinct_normalized_content() {
        let absent: AdUpdate = serde_json::from_str(r#"{"name":"Ad"}"#).unwrap();
        let clear: AdUpdate =
            serde_json::from_str(r#"{"name":"Ad","advertised_product_id":null}"#).unwrap();
        assert_ne!(
            serde_json::to_value(absent).unwrap(),
            serde_json::to_value(clear).unwrap()
        );
        assert!(serde_json::from_str::<AdUpdate>(r#"{"name":null}"#).is_err());
    }
    #[test]
    fn attribution_links_reserved_parameter_and_canonical_expansion_bound() {
        for value in [
            "https://shop.merchant.com/?ob_click=x",
            "https://shop.merchant.com/?%6fb_click=x",
        ] {
            assert!(link_destination(value).is_err());
        }
        let value = format!("https://shop.merchant.com/{}", "é".repeat(700));
        assert!(value.len() < 2048);
        assert!(link_destination(&value).is_err());
        assert_eq!(
            link_destination("https://shop.merchant.com/?x=1&x=2#part").unwrap(),
            "https://shop.merchant.com/?x=1&x=2#part"
        );
    }
}
