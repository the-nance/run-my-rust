use serde::{Deserialize, Serialize};

use std::{fs::read_to_string, lazy::SyncLazy};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub token: String,
    pub channels: Vec<u64>,
	pub banned_roles: Vec<u64>
}

pub static CONFIG: SyncLazy<Config> = SyncLazy::new(|| {
    let contents = read_to_string("config.toml").expect("Could not open config.toml");
    toml::from_str(&contents).unwrap()
});
