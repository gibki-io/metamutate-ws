use serde::{Deserialize, Serialize};

pub struct _MetadataRoot {
    pub root: MetadataInner
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MetadataInner {
    pub name: String,
    pub symbol: String,
    pub description: String,
    pub seller_fee_basis_points: u16,
    pub image: String,
    pub external_url: String,
    pub attributes: Vec<MetadataAttribute>,
    pub properties: MetadataProperties,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MetadataAttribute {
    pub trait_type: String,
    pub value: String
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MetadataProperties {
    pub files: Vec<PropertyFiles>,
    pub category: String,
    pub creators: Vec<PropertyCreators>
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PropertyCreators {
    pub address: String,
    pub share: u16
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PropertyFiles {
    pub uri: String,
    #[serde(rename = "type")]
    pub typee: String // type, change with serde
}

#[derive(Deserialize, Serialize)]
pub struct MetadataStatus {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub typee: String
}