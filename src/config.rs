use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use std::{fs::read_to_string};

#[derive(Debug, Serialize, Deserialize)]
pub struct ButtonMenuRole {
	pub id: u64,
	pub label: String,
	pub style: u8
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ButtonMenu {
	pub channel_id: u64,
	pub roles: Vec<ButtonMenuRole>,
	pub message: String,
	pub comp_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ButtonMenuSettings {
	pub send_on_start: bool,
	pub messages_to_check: u64
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub token: String,
	pub bot_id: u64,
    pub channels: Vec<u64>,
    pub banned_roles: Vec<u64>,
	pub settings: ButtonMenuSettings,
	pub button_menus: Vec<ButtonMenu>
}

pub static CONFIG: Lazy<Config> = Lazy::new(|| {
    let contents = read_to_string("config.toml").expect("Could not open config.toml");
    toml::from_str(&contents).unwrap()
});
