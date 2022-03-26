// use futures_util::{TryStreamExt,StreamExt};
use futures::StreamExt;

use hyper::body::Buf;
use hyper::{client::HttpConnector, Body, Client as HyperClinet, Request};
use hyper_tls::HttpsConnector;
use twilight_http::request::AuditLogReason;
use std::{
    error::Error,
    sync::{Arc},
};

use tokio::{task};
use tracing::{error, info};
use twilight_cache_inmemory::{InMemoryCache, InMemoryCacheBuilder, ResourceType};
use twilight_gateway::{cluster::ClusterBuilder, Event, Intents};
use twilight_http::{request::channel::reaction::RequestReactionType, Client};
use twilight_model::{
    id::{
        marker::{RoleMarker, ChannelMarker,ApplicationMarker},
        Id,
    },
	application::{component::{Component,action_row::ActionRow,button::{Button,ButtonStyle}}},
	http::interaction::{InteractionResponse,InteractionResponseType},
	channel::message::MessageFlags
};

mod config;
mod play;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("Starting up bot");

    let token = config::CONFIG.token.clone();

    let (cluster, mut events) = ClusterBuilder::new(
        token.clone(),
        Intents::GUILD_BANS | Intents::GUILDS | Intents::GUILD_MESSAGES | Intents::GUILD_MEMBERS,
    )
    .build()
    .await?;

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
    let http = Arc::new(Client::builder().token(token).build());
    let hyper_cline =
        Arc::new(HyperClinet::builder().build::<_, hyper::Body>(HttpsConnector::new()));
	
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

    tokio::spawn(async move {
        cluster.up().await;
    });

    while let Some(event) = events.next().await {
		cache.update(&event.1);
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
