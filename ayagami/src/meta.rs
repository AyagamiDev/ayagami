use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct FileReferences {
    pub moc: String,
    pub textures: Vec<String>,
    pub physics: Option<String>,
    pub display_info: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Model3 {
    pub version: u32,
    pub file_references: FileReferences,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Parameter {
    pub id: String,
    pub group_id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct ParameterGroup {
    pub id: String,
    pub group_id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Part {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct DisplayInfo {
    pub version: u32,
    pub parameters: Vec<Parameter>,
    pub parameter_groups: Vec<ParameterGroup>,
    pub parts: Vec<Part>,
}
