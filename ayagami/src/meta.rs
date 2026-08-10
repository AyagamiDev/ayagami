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
pub struct NamedEntity {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct DisplayInfo {
    pub version: u32,
    pub parameters: Vec<Parameter>,
    pub parameter_groups: Vec<ParameterGroup>,
    pub parts: Vec<NamedEntity>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsForces {
    pub gravity: Vec2,
    pub wind: Vec2,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsMeta {
    pub fps: Option<f32>,
    pub effective_forces: PhysicsForces,
    pub physics_dictionary: Vec<NamedEntity>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum TargetType {
    Parameter,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsTarget {
    pub target: TargetType,
    pub id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum PhysicsType {
    X,
    Angle,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsInput {
    pub source: PhysicsTarget,
    pub weight: f32,
    #[serde(rename = "Type")]
    pub input_type: PhysicsType,
    pub reflect: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsOutput {
    pub destination: PhysicsTarget,
    pub vertex_index: u32,
    pub scale: f32,
    pub weight: f32,
    // Always Angle
    // #[serde(rename = "Type")]
    // pub output_type: PhysicsType,
    pub reflect: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsVertex {
    // Ignored
    //pub position: Vec2,
    pub mobility: f32,
    pub delay: f32,
    pub acceleration: f32,
    pub radius: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsRange {
    pub minimum: f32,
    pub default: f32,
    pub maximum: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsNormalization {
    pub position: PhysicsRange,
    pub angle: PhysicsRange,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PhysicsSetting {
    pub id: String,
    pub input: Vec<PhysicsInput>,
    pub output: Vec<PhysicsOutput>,
    pub vertices: Vec<PhysicsVertex>,
    pub normalization: PhysicsNormalization,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct Physics3 {
    pub version: u32,
    pub meta: PhysicsMeta,
    pub physics_settings: Vec<PhysicsSetting>,
}
