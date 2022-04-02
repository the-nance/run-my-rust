// use futures_util::{TryStreamExt,StreamExt};
use futures::StreamExt;

use hyper::body::Buf;
use hyper::{client::HttpConnector, Body, Client as HyperClinet, Request};
use hyper_tls::HttpsConnector;
use tracing_subscriber::FmtSubscriber;
use twilight_http::request::AuditLogReason;
use twilight_model::application::component::text_input::TextInputStyle;
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::id::marker::{GuildMarker, MessageMarker};
use twilight_util::builder::embed::{EmbedBuilder, ImageSource};
use std::{
    error::Error,
    sync::{Arc},
};

use tokio::{task};
use tracing::{error, info, Level};
use twilight_cache_inmemory::{InMemoryCache, InMemoryCacheBuilder, ResourceType};
use twilight_gateway::{cluster::ClusterBuilder, Event, Intents};
use twilight_http::{request::channel::reaction::RequestReactionType, Client};
use twilight_model::{
    id::{
        marker::{RoleMarker, ChannelMarker,ApplicationMarker},
        Id,
    },
	application::{component::{Component,action_row::ActionRow,button::{Button,ButtonStyle},TextInput},command::{CommandType,CommandOption, ChoiceCommandOptionData, BaseCommandOptionData}},
	http::{interaction::{InteractionResponse,InteractionResponseType,InteractionResponseData}, attachment::Attachment},
	channel::message::MessageFlags
};
use twilight_util::builder::{command::{BooleanBuilder, CommandBuilder, StringBuilder},embed::EmbedAuthorBuilder};

mod config;
mod play;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

	let subscriber = FmtSubscriber::builder()
		.with_max_level(Level::INFO)
		.finish();

	tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    info!("Starting up bot");

    let token = config::CONFIG.token.clone();

    let cache = Arc::new(
        InMemoryCacheBuilder::new()
            .resource_types(
                ResourceType::ROLE
                    | ResourceType::USER_CURRENT
                    | ResourceType::MEMBER
                    | ResourceType::GUILD,
            )
            .build(),
    );
    let http = Arc::new(Client::builder().token(token.clone()).build());

	let commands = &[
		CommandBuilder::new(
			"run".into(),
			"Run some Rust out side of defined run-my-rust channels.".into(),
			CommandType::ChatInput,
		).guild_id(Id::<GuildMarker>::new(config::CONFIG.server_id)).default_permission(true).build(),
		CommandBuilder::new(
			"project".into(),
			"Show off a neat project of yours!".into(),
			CommandType::ChatInput,
		)
		.option(
			CommandOption::String(ChoiceCommandOptionData {
			    autocomplete: false,
			    choices: vec![],
			    description: String::from("Whats the name of your project?"),
			    name: String::from("name"),
			    required: true,
			})
		)
		.option(
			CommandOption::String(ChoiceCommandOptionData {
			    autocomplete: false,
			    choices: vec![],
			    description: String::from("Whats your project's description? Max of 1000 characters. Keep it short and simple."),
			    name: String::from("description"),
			    required: false,
			})
		)
		.option(
			CommandOption::Boolean(BaseCommandOptionData {
			    description: String::from("Is this project on crates.io? Make sure the name you set is the same on crates.io"),
			    name: String::from("crates-io"),
			    required: false,
			})
		)

		.option(
			CommandOption::String(ChoiceCommandOptionData {
			    autocomplete: false,
			    choices: vec![],
			    description: String::from("Does your project have a github repo?"),
			    name: String::from("github"),
			    required: false,
			}))
		.guild_id(Id::<GuildMarker>::new(config::CONFIG.server_id)).default_permission(true).build(),
		CommandBuilder::new(
			"run-message".into(),
			"".into(),
			CommandType::Message,
		).guild_id(Id::<GuildMarker>::new(config::CONFIG.server_id)).default_permission(true).build()
	];
	http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
		.set_guild_commands(Id::<GuildMarker>::new(config::CONFIG.server_id), commands).exec().await.unwrap();

    let hyper_cline =
        Arc::new(HyperClinet::builder().build::<_, hyper::Body>(HttpsConnector::new()));
	
	let (cluster, mut events) = ClusterBuilder::new(
			token,
			Intents::GUILDS
			| Intents::GUILD_MEMBERS
			| Intents::GUILD_MESSAGES
			| Intents::MESSAGE_CONTENT
		)
		.http_client(http.clone())
		.build()
		.await?;
	
	//tokio::spawn(async move {
	cluster.up().await;
	//});
	

	let http_clone = http.clone();
	task::spawn(async move {
		let http = http_clone.clone();

		if config::CONFIG.settings.send_on_start {
			for button_menu in &config::CONFIG.button_menus {
				let channel_id = Id::<ChannelMarker>::new(button_menu.channel_id);
				let channel_messages = http
					.channel_messages(channel_id)
					.limit(config::CONFIG.settings.messages_to_check as u16).unwrap()
					.exec()
					.await.unwrap();

				for message in channel_messages.models().await.unwrap() {
					if message.author.bot && message.author.id.get() == config::CONFIG.bot_id {
						if message.content == button_menu.message {
							match http.delete_message(channel_id, message.id).reason("Sending new button menu").unwrap().exec().await {
								Ok(_) => info!("Should've deleted?"),
								Err(_) => error!("Failed to delete a bot's message while starting"),
							}
						}
						continue;
					}

					match http.create_message(channel_id)
						.content(&button_menu.message).unwrap()
						.components(&[
							Component::ActionRow(
								ActionRow {
									components: button_menu.roles.iter().map(|r| {
										let role_label = format!("{}", r.id);
										Component::Button(Button {
									        custom_id: Some(role_label),
									        disabled: false,
									        emoji: None,
									        label: Some(r.label.clone()),
									        style: match r.style {
												1 => ButtonStyle::Primary,
												2 => ButtonStyle::Secondary,
												3 => ButtonStyle::Success,
												4 => ButtonStyle::Danger,
												_ => ButtonStyle::Secondary
											},
									        url: None,
									    })
									}).collect::<Vec<Component>>()
								})]
						).unwrap().exec().await {
					        Ok(_) => {},
					        Err(_) => error!("Failed to send a button menu."),
					    }
					break;
				}
				
			}
		}
	});

    while let Some(event) = events.next().await {
		// cache.update(&event.1);
        let cache = cache.clone();
        let http = http.clone();
        let hyper = hyper_cline.clone();

        tokio::spawn(async move {
            if handle_event(cache, http.clone(), event.clone(), hyper).await.is_err() {
                error!("Error handling event");
				match event.1 {
					Event::MessageCreate(message) => {
						let _message = http
							.create_message(message.channel_id)
							.content(":x: there was an error handling that event. please repooort me.")
							.unwrap()
							.exec()
							.await;
					}
					_ => error!("Error from event we dont even handle...")
				}
            };
        });
    }
    Ok(())
}

async fn handle_event(
    cache: Arc<InMemoryCache>,
    http: Arc<Client>,
    (shard_id, event): (u64, Event),
    http2: Arc<HyperClinet<HttpsConnector<HttpConnector>>>,
) -> Result<(), Box<dyn Error>> {
    cache.update(&event);

    match event {
        Event::Ready(_) => {
            info!("Shard {} is now ready", shard_id);
        }
		Event::InteractionCreate(interaction) => {
			match &interaction.0 {
				twilight_model::application::interaction::Interaction::ModalSubmit(modal) => {
					let guild_id = match interaction.guild_id() {
				        Some(g) =>g,
				        None => {return Ok(())}
				    };

					// println!("{:#?}", modal);
					http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
						.create_response(
							modal.id, 
							&modal.token,
							&InteractionResponse {
								kind: InteractionResponseType::DeferredChannelMessageWithSource,
								data: None,
				
							}
						)
						.exec().await.unwrap();

					if let Some(member) = &modal.member {
						if let Some(user) = &member.user {
							if cache.member(guild_id, user.id).is_none() {
								http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
									.create_followup(
										&modal.token,
									)
									.content("You're not cached. Send a message somewhere and press me again.").unwrap()
									.flags(MessageFlags::EPHEMERAL)
									.exec()
									.await.unwrap();

								return Ok(());							
							}

							let member = cache.member(guild_id, user.id).unwrap();
							for role in &config::CONFIG.banned_roles {
								if member.roles().contains(&Id::<RoleMarker>::new(*role)) {
									info!("Banned user tried running getting a role");
									return Ok(());
								}
							}

							let mut comps = modal.data.components.iter();
							while let Some(action_row) = comps.next() {
								for comp in &action_row.components {
									if modal.data.custom_id == String::from("run-my-rust") {
										match comp.custom_id.as_str() {
											"code-to-run" => {
												let content = comp.value.clone();
												let playground = play::Playground {
													channel: "stable".to_string(),
													mode: "debug".to_string(),
													edition: "2021".to_string(),
													backtrace: false,
													tests: false,
													crate_type: "bin".to_string(),
													code: if content.contains("fn main()") {
														content
													} else {
														format!(
															"fn main() {{ println!(\"{{:?}}\", {{ {} }} ) }}",
															content
														)
													},
												};
					
												let request = Request::builder()
													.uri("https://play.rust-lang.org/execute")
													.method("POST")
													.header("User-Agent", "RunMyRust/1.0")
													.header("Content-Type", "application/json")
													.body(Body::from(serde_json::to_vec(&playground)?))?;
					
												let response = match http2.request(request).await {
													Ok(r) => r,
													Err(_) => return Ok(())
												};
					
												let body = hyper::body::aggregate(response).await?;
												let response: play::PlaygroundResult = serde_json::from_reader(body.reader())?;

												let content = &format!(
													"Result: {}\nStds:\n**Out:** {}\n**Err:** ```{}```",
													response.success,
													response.stdout,
													response.stderr.replace('`', "\"")
												);
												
												if content.len() > 2000 {
													http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
													.create_followup(
														&modal.token,
													)
													.content("<:ferrisbanne:958831785922416780> The output was too long. So here you go. Have an attachment. Is this what you wanted?").unwrap()
													.attachments(&[
														Attachment { 
															description: None, 
															file: content.clone().as_bytes().to_vec(), 
															filename: format!("{}-{}.txt", guild_id.get(), user.id.get())
														}
													]).unwrap()
													.exec()
													.await.unwrap();
												} else {
													http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
													.create_followup(
														&modal.token,
													)
													.content(content).unwrap()
													.exec()
													.await.unwrap();
												}				
											}
											_ => continue
										}
									}
								}
							}
						}
					}
				}
				twilight_model::application::interaction::Interaction::ApplicationCommand(cmd) => {
					match cmd.data.name.as_str() {
						"project" => {
							let guild_id = match interaction.guild_id() {
								Some(g) =>g,
								None => {return Ok(())}
							};

							if let Some(channel_id) = config::CONFIG.projects_channel {						
								if let Some(member) = &cmd.member {
									if let Some(user) = &member.user {
										if cache.member(cmd.guild_id.unwrap(), user.id).is_none() {
											http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
												.create_followup(
													&cmd.token,
												)
												.content("You're not cached. Send a message somewhere and press me again.").unwrap()
												.flags(MessageFlags::EPHEMERAL)
												.exec()
												.await.unwrap();
			
											return Ok(());							
										}

										let member = cache.member(guild_id, user.id).unwrap();
										for role in &config::CONFIG.banned_roles {
											if member.roles().contains(&Id::<RoleMarker>::new(*role)) {
												info!("Banned user tried running getting a role");
												return Ok(());
											}
										}
		

										http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
										.create_response(
											cmd.id, 
											&cmd.token,
											&InteractionResponse {
												kind: InteractionResponseType::DeferredChannelMessageWithSource,
												data: None,
								
											}
										)
										.exec().await.unwrap();

										let name = match &cmd.data.options.iter().filter(|e| e.name == "name" ).next().unwrap().value {
											CommandOptionValue::String(string) => string,
											_ => {
												http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
													.create_followup(
														&cmd.token,
													)
													.content("That name you provided was mangled by Discord?").unwrap()
													.flags(MessageFlags::EPHEMERAL)
													.exec()
													.await.unwrap();
												return Ok(())
											}
										};

										let github = cmd.data.options.iter().filter(|e| e.name == "github" ).next();
										let description = cmd.data.options.iter().filter(|e| e.name == "description" ).next();
										let crates_io = cmd.data.options.iter().filter(|e| e.name == "crates-io" ).next();
										
										let mut comps = Vec::new();

										if let Some(crates_io) = crates_io {
											match crates_io.value {
									            CommandOptionValue::Boolean(crates) => if crates {
													comps.push(Component::Button(Button {
										                custom_id: None,
										                disabled: false,
										                emoji: None,
										                label: Some("view on crates.io".to_string()),
										                style: ButtonStyle::Link,
										                url: Some(format!("https://crates.io/crates/{}", name)),
										            }))
												},
									            _ => {}
									        }
										}

										if let Some(github) = github {
											match &github.value {
									            CommandOptionValue::String(github) => {
													comps.push(Component::Button(Button {
										                custom_id: None,
										                disabled: false,
										                emoji: None,
										                label: Some("view on github".to_string()),
										                style: ButtonStyle::Link,
										                url: Some(github.to_string()),
										            }))
												},
									            _ => {}
									        }											
										}

										let author = EmbedAuthorBuilder::new(format!("{}#{}", user.name, user.discriminator()));
										
										let author = if let Some(avatar) = user.avatar {
											match ImageSource::url(format!(
												"https://cdn.discordapp.com/avatars/{}/{}.png",
												user.id, avatar
											)) {
												Ok(icon_url) => {
													author.icon_url(icon_url)
												},
												Err(_) => author
											}
										} else { author };

										let embed = EmbedBuilder::new()
											.author(author.build())
											.title(name)
											.color(11237454);

										let embed = if let Some(description) = description {
											match &description.value {
									            CommandOptionValue::String(description) => {
													if description.len() > 1000 || description.len() < 1 {
														http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
														.create_followup(
															&cmd.token,
														)
														.content("Sorry your description is either too large or too small. Remmber it cannot be above 1000 characters and must be above 1 character.").unwrap()
														.flags(MessageFlags::EPHEMERAL)
														.exec()
														.await.unwrap();
														return Ok(())
													}

													embed.description(description)
												},
									            _ => embed
									        }
										} else {
											embed
										};
										let embed = embed.validate().unwrap();
										let embed = vec![embed.build()];

										match http.create_message(Id::<ChannelMarker>::new(channel_id))
											.embeds(&embed).unwrap()
											.content("A new project has been discovered!").unwrap()
											.components(&[Component::ActionRow(ActionRow {
												components: comps
											})]).unwrap()
											.exec().await {
												Ok(message) => match message.model().await {
													Ok(m) => {
														match http.create_thread_from_message(m.channel_id, m.id, &name).unwrap().exec().await {
															_ => {}
														}
													},
													Err(_) => {}
												}
												Err(_) => {}
											}
										
										http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
											.create_followup(
												&cmd.token,
											)
											.content("👍").unwrap()
											.flags(MessageFlags::EPHEMERAL)
											.exec()
											.await.unwrap();
									}
								}
							}
						}
 						"run-message" => {
							let guild_id = match interaction.guild_id() {
								Some(g) =>g,
								None => {return Ok(())}
							};

							http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
								.create_response(
									cmd.id, 
									&cmd.token,
									&InteractionResponse {
										kind: InteractionResponseType::DeferredChannelMessageWithSource,
										data: None,
						
									}
								)
								.exec().await.unwrap();

							
							let loading = RequestReactionType::Unicode { name: "🌀" };
							let failed = RequestReactionType::Unicode { name: "❌" };
							let success = RequestReactionType::Unicode { name: "✅" };
							let warning = RequestReactionType::Unicode { name: "⚠️" };

							if let (Some(target_id), Some(messages)) = (cmd.data.target_id,cmd.data.resolved.as_ref()) {
								if cmd.guild_id.is_none() {
									http.create_reaction(cmd.channel_id, target_id.cast::<MessageMarker>(), &warning)
										.exec()
										.await?;
										return Ok(())
								}

								let message = match messages.messages.get(&target_id.cast::<MessageMarker>()) {
									Some(message) => {
										message
									},
									None => {
										http.create_reaction(cmd.channel_id, target_id.cast::<MessageMarker>(), &warning)
										.exec()
										.await?;
										return Ok(())
									},
								};
								
								if let Some(member) = &cmd.member {
									if let Some(user) = &member.user {
										if cache.member(cmd.guild_id.unwrap(), user.id).is_none() {
											http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
												.create_followup(
													&cmd.token,
												)
												.content("You're not cached. Send a message somewhere and press me again.").unwrap()
												.flags(MessageFlags::EPHEMERAL)
												.exec()
												.await.unwrap();
			
											return Ok(());							
										}
										
										let member = cache.member(cmd.guild_id.unwrap(), user.id).unwrap();
										for role in &config::CONFIG.banned_roles {
											if member.roles().contains(&Id::<RoleMarker>::new(*role)) {
												info!("Banned user tried running getting a role");
												return Ok(());
											}
										}

										let mut message_content = message.content.split("```").peekable();
										if message_content.peek().is_some() && message_content.next().unwrap().is_empty() {
											if let Some(content) = message_content.next() {
												let mut splitted_newlines = content.split('\n').peekable();
												// If the message code black isn't `rs` or `rust` then just ignore it.
												if splitted_newlines.peek().is_some()
													&& splitted_newlines.next().unwrap()  == "rs"
													|| splitted_newlines.next().unwrap() == "rust"
												{
													http.create_reaction(cmd.channel_id, target_id.cast::<MessageMarker>(), &loading)
														.exec()
														.await?;
						
													let playground = play::Playground {
														channel: "stable".to_string(),
														mode: "debug".to_string(),
														edition: "2021".to_string(),
														backtrace: false,
														tests: false,
														crate_type: "bin".to_string(),
														code: if content.contains("fn main()") {
															splitted_newlines.collect::<Vec<&str>>().join("\n")
														} else {
															format!(
																"fn main() {{ println!(\"{{:?}}\", {{ {} }} ) }}",
																splitted_newlines.collect::<Vec<&str>>().join("\n")
															)
														},
													};
						
													let request = Request::builder()
														.uri("https://play.rust-lang.org/execute")
														.method("POST")
														.header("User-Agent", "RunMyRust/1.0")
														.header("Content-Type", "application/json")
														.body(Body::from(serde_json::to_vec(&playground)?))?;
						
													let response = match http2.request(request).await {
														Ok(r) => r,
														Err(_) => {
															http.create_reaction(cmd.channel_id, target_id.cast::<MessageMarker>(), &failed)
																.exec()
																.await?;
															return Ok(());
														}
													};
						
													let body = hyper::body::aggregate(response).await?;
													let response: play::PlaygroundResult =
														serde_json::from_reader(body.reader())?;
													// if response.success == false {
													http.create_reaction(cmd.channel_id, target_id.cast::<MessageMarker>(), &success)
														.exec()
														.await?;
													
													let content = &format!(
														"Result: {}\nStds:\n**Out:** {}\n**Err:** ```{}```",
														response.success,
														response.stdout,
														response.stderr.replace('`', "\"")
													);
		
													if content.len() > 2000 {
														http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
														.create_followup(
															&cmd.token,
														)
														.content("<:ferrisbanne:958831785922416780> The output was too long. So here you go. Have an attachment. Is this what you wanted?").unwrap()
														.attachments(&[
															Attachment { 
																description: None, 
																file: content.clone().as_bytes().to_vec(), 
																filename: format!("{}-{}.txt", cmd.guild_id.unwrap().get(), user.id.get())
															}
														]).unwrap()
														.exec()
														.await.unwrap();
													} else {
														http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
														.create_followup(
															&cmd.token,
														)
														.content(content).unwrap()
														.exec()
														.await.unwrap();
													}	
												}
											}

									}
								}
							}
							}													
						}
						"run" => {
							let guild_id = match interaction.guild_id() {
								Some(g) =>g,
								None => {return Ok(())}
							};


							if let Some(member) = &cmd.member {
								if let Some(user) = &member.user {
									if cache.member(guild_id, user.id).is_none() {
										http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
											.create_followup(
												&cmd.token,
											)
											.content("You're not cached. Send a message somewhere and press me again.").unwrap()
											.flags(MessageFlags::EPHEMERAL)
											.exec()
											.await.unwrap();
		
										return Ok(());							
									}
		
									let member = cache.member(guild_id, user.id).unwrap();
									for role in &config::CONFIG.banned_roles {
										if member.roles().contains(&Id::<RoleMarker>::new(*role)) {
											info!("Banned user tried running getting a role");
											return Ok(());
										}
									}
			
									http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
										.create_response(
											cmd.id,
											&cmd.token,
											&InteractionResponse { 
												kind: InteractionResponseType::Modal,
												data: Some(InteractionResponseData {
										            allowed_mentions: None,
										            attachments: None,
										            choices: None,
										            components: Some(vec![
														Component::ActionRow(ActionRow {
															components: vec![
																Component::TextInput(TextInput { 
																	custom_id: String::from("code-to-run"), 
																	label: String::from("Code to run"), 
																	max_length: None, 
																	min_length: None, 
																	placeholder: None, 
																	required: Some(true), 
																	style: TextInputStyle::Paragraph, 
																	value: None
																})
															]
														})
													]),
										            content: None,
										            custom_id: Some(String::from("run-my-rust")),
										            embeds: None,
										            flags: None,
										            title: Some(String::from("Rust Runner 9000")),
										            tts: Some(false),
										        })			
											}
										)
										.exec()
										.await.unwrap();							
									}
									
							}
						}
						_ => {}
					}
				}
			    twilight_model::application::interaction::Interaction::MessageComponent(msgcmp) => {
					http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
						.create_response(
							msgcmp.id, 
							&msgcmp.token,
							&InteractionResponse {
								kind: InteractionResponseType::DeferredUpdateMessage,
								data: None,
				
					        }
						)
						.exec()
						.await.unwrap();

					let guild_id = match interaction.guild_id() {
				        Some(g) =>g,
				        None => {return Ok(())}
				    };
					
					if let Some(member) = &msgcmp.member {
						if let Some(user) = &member.user {
							if cache.member(guild_id, user.id).is_none() {
								http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
									.create_followup(
										&msgcmp.token,
									)
									.content("You're not cached. Send a message somewhere and press me again.").unwrap()
									.flags(MessageFlags::EPHEMERAL)
									.exec()
									.await.unwrap();

								return Ok(());							
							}

							let member = cache.member(guild_id, user.id).unwrap();
	
							for role in &config::CONFIG.banned_roles {
								if member.roles().contains(&Id::<RoleMarker>::new(*role)) {
									info!("Banned user tried running getting a role");
									return Ok(());
								}
							}
	
							let role_id = match msgcmp.data.custom_id.parse::<u64>() {
								Ok(id) => Id::<RoleMarker>::new(id),
								Err(_) => {
									http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
										.create_followup(
											&msgcmp.token,
										)
										.content("Failed to get the role id. Sorry.").unwrap()
										.flags(MessageFlags::EPHEMERAL)
										.exec()
										.await.unwrap();

									return Ok(())
								}
							};
	
							let mut roles = member.roles().to_vec().clone();
	
							let message = if roles.contains(&role_id) {							
								let new_roles = roles.iter().filter(|e| *e != &role_id).copied().collect::<Vec<Id<RoleMarker>>>();
								http.update_guild_member(guild_id, user.id)
									.roles(&new_roles).exec().await.unwrap();
									format!("<:ferrischeck:957417376314429490> removed <@&{}>", role_id.get())
							} else {
								roles.push(role_id);
								http.update_guild_member(guild_id, user.id)
									.roles(&roles).exec().await.unwrap();
								format!("<:ferrischeck:957417376314429490> added <@&{}>", role_id.get())
							};
							
							http.interaction(Id::<ApplicationMarker>::new(config::CONFIG.bot_id))
								.create_followup(
									&msgcmp.token,
								)
								.content(&message).unwrap()
								.flags(MessageFlags::EPHEMERAL)
								.exec()
								.await.unwrap();
						}
					}
				},
				_ => {}
			}
		}
        Event::MessageCreate(message) => {
            if config::CONFIG.channels.contains(&message.channel_id.get()) {
                if let Some(member) = &message.member {
                    for role in &config::CONFIG.banned_roles {
                        if member.roles.contains(&Id::<RoleMarker>::new(*role)) {
                            info!("Banned user ({:?}) tried running rust in guild: {:?}, channel: {:?}", 
								message.author.id,
								message.guild_id,
								message.channel_id
							);
                            return Ok(());
                        }
                    }
                }

                let loading = RequestReactionType::Unicode { name: "🌀" };
                let failed = RequestReactionType::Unicode { name: "❌" };
                let success = RequestReactionType::Unicode { name: "✅" };

                let mut message_content = message.content.split("```").peekable();
                if message_content.peek().is_some() && message_content.next().unwrap().is_empty() {
                    if let Some(content) = message_content.next() {
                        let mut splitted_newlines = content.split('\n').peekable();
                        // If the message code black isn't `rs` or `rust` then just ignore it.
                        if splitted_newlines.peek().is_some()
                            && splitted_newlines.next().unwrap()  == "rs"
                            || splitted_newlines.next().unwrap() == "rust"
                        {
                            http.create_reaction(message.channel_id, message.id, &loading)
                                .exec()
                                .await?;

                            let playground = play::Playground {
                                channel: "stable".to_string(),
                                mode: "debug".to_string(),
                                edition: "2021".to_string(),
                                backtrace: false,
                                tests: false,
                                crate_type: "bin".to_string(),
                                code: if content.contains("fn main()") {
                                    splitted_newlines.collect::<Vec<&str>>().join("\n")
                                } else {
                                    format!(
                                        "fn main() {{ println!(\"{{:?}}\", {{ {} }} ) }}",
                                        splitted_newlines.collect::<Vec<&str>>().join("\n")
                                    )
                                },
                            };

                            let request = Request::builder()
                                .uri("https://play.rust-lang.org/execute")
                                .method("POST")
                                .header("User-Agent", "RunMyRust/1.0")
                                .header("Content-Type", "application/json")
                                .body(Body::from(serde_json::to_vec(&playground)?))?;

                            let response = match http2.request(request).await {
                                Ok(r) => r,
                                Err(_) => {
                                    http.create_reaction(message.channel_id, message.id, &failed)
                                        .exec()
                                        .await?;
                                    return Ok(());
                                }
                            };

                            let body = hyper::body::aggregate(response).await?;
                            let response: play::PlaygroundResult =
                                serde_json::from_reader(body.reader())?;
                            println!("{:#?}", response);
                            // if response.success == false {
                            http.create_reaction(message.channel_id, message.id, &success)
                                .exec()
                                .await?;

                            let new_thread_channel = http
                                .create_thread_from_message(
                                    message.channel_id,
                                    message.id,
                                    &format!(
                                        "{}-{}",
                                        message.author.id,
                                        message.id
                                    ),
                                )?
                                .exec()
                                .await?
                                .model()
                                .await?;

                            let message = http
                                .create_message(new_thread_channel.id)
                                .content(&format!(
                                    "Result: {}\nStds:\n**Out:** {}\n**Err:** ```{}```",
                                    response.success,
                                    response.stdout,
                                    response.stderr.replace('`', "\"")
                                ))?
                                .exec()
                                .await;

                            match message {
                                Ok(_) => info!("Successfully created a thread."),
                                Err(_) => error!("Failed to created a thread."),
                            }

                            println!("{:?}", playground);
                            //}

                            return Ok(());
                        }

                        http.create_reaction(message.channel_id, message.id, &failed)
                            .exec()
                            .await?;
                    }
                }
            }
        }
        _ => {}
    }

    Ok(())
}
