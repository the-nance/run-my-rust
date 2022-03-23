use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Playground {
    pub channel: String,
    pub mode: String,
    pub edition: String,
    // Both of these values will always be `false`. just fyi.
    pub backtrace: bool,
    pub tests: bool,
    #[serde(rename = "crateType")]
    pub crate_type: String,
    pub code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaygroundResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}
