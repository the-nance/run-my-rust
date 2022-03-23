#![feature(once_cell)]

use futures::{future::FutureExt,Stream};
// use futures_util::{TryStreamExt,StreamExt};
use futures::StreamExt;
use hyper::body::Buf;
use std::{
    error::Error,
    io::{Read, Write},
    sync::{Arc,Mutex},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{mpsc},
    task,
};
use futures_util::{
    future, pin_mut,
    stream::{SplitSink},
    SinkExt,
};
use tracing::{error, info};
use twilight_cache_inmemory::{InMemoryCache, InMemoryCacheBuilder, ResourceType};
use twilight_gateway::{cluster::ClusterBuilder, Event, Intents};
use twilight_http::{request::channel::reaction::RequestReactionType, Client};
use twilight_model::{
    guild::{audit_log::AuditLogEventType, Permissions},
    id::{
        marker::{GuildMarker, UserMarker},
        Id,
    },
};
use hyper::{Client as HyperClinet,Body, client::HttpConnector, Request, body::HttpBody};
use hyper_tls::HttpsConnector;

mod play;
mod config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    info!("Starting up bot");

    let token = config::CONFIG.token.clone();

    let (cluster, mut events) = ClusterBuilder::new(token.clone(), Intents::GUILD_BANS | Intents::GUILDS | Intents::GUILD_MESSAGES).build().await?;
    
	let cache = Arc::new(InMemoryCacheBuilder::new().resource_types(ResourceType::ROLE | ResourceType::USER_CURRENT | ResourceType::MEMBER | ResourceType::GUILD ).build());
    let http = Arc::new(Client::builder().token(token).build());
	let hyper_cline = Arc::new(HyperClinet::builder().build::<_, hyper::Body>(HttpsConnector::new()));

    tokio::spawn(async move {
        cluster.up().await;
    });


    while let Some(event) = events.next().await {
        let cache = cache.clone();
        let http = http.clone();
		let hyper = hyper_cline.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_event(cache, http, event, hyper).await {
                error!("Error handling event: {}", e);
            };
        });
    }
	Ok(())
}

async fn handle_event(cache: Arc<InMemoryCache>, http: Arc<Client>, (shard_id, event): (u64, Event), http2: Arc<HyperClinet<HttpsConnector<HttpConnector>>>) -> Result<(), Box<dyn Error>> {
    cache.update(&event);

    match event {
        Event::Ready(_) => {
            info!("Shard {} is now ready", shard_id);
        }
        Event::MessageCreate(message) => {
			if config::CONFIG.channels.contains(&message.channel_id.get()) {
				let loading = RequestReactionType::Unicode { name: "🌀" };
				let failed = RequestReactionType::Unicode { name: "❌" };
				let success = RequestReactionType::Unicode { name: "✅" };

				let mut message_content = message.content.split("```").peekable();
				if message_content.peek().is_some() && message_content.next().unwrap().is_empty() {
					if let Some(content) = message_content.next() {
						let mut splitted_newlines = content.split("\n").peekable();
						// If the message code black isn't `rs` or `rust` then just ignore it.
						if splitted_newlines.peek().is_some() && splitted_newlines.next().unwrap() == "rs" || splitted_newlines.next().unwrap() == "rust" {
							http
							.create_reaction(message.channel_id, message.id, &loading)
							.exec()
							.await.unwrap();	

							let playground = play::Playground {
						        channel: "stable".to_string(),
						        mode: "debug".to_string(),
						        edition: "2021".to_string(),
						        backtrace: false,
						        tests: false,
						        crate_type: "bin".to_string(),
								code: splitted_newlines.collect::<Vec<&str>>().join("\n")
						    };
							
							let request = Request::builder()
								.uri("https://play.rust-lang.org/execute")
								.method("POST")
								.header("User-Agent", "RunMyRust/1.0")
								.header("Content-Type", "application/json")
								.body(Body::from(serde_json::to_vec(&playground).unwrap()))
								.unwrap();

							let response = match http2.request(request).await {
								Ok(r) => r,
								Err(_) => {
									http
									.create_reaction(message.channel_id, message.id, &failed)
									.exec()
									.await.unwrap();
									return Ok(());
								}
							};
							
							let body = hyper::body::aggregate(response).await.unwrap();
							let response: play::PlaygroundResult= serde_json::from_reader(body.reader()).unwrap();

							// if response.success == false {
								http
								.create_reaction(message.channel_id, message.id, &success)
								.exec()
								.await.unwrap();
								
								let new_thread_channel = http
									.create_thread_from_message(
										message.channel_id, 
										message.id, 
										&format!("{}-{}", message.author.id.to_string(), message.id.to_string())
									).unwrap()
									.exec()
									.await.unwrap().model().await.unwrap();

								let message = http
									.create_message(new_thread_channel.id())
									.content(&format!("Result: {}\nStds:\n**Out:** ```{}```\n**Err:** ```{}```", response.success, response.stdout, response.stderr)).unwrap()
									.exec()
									.await;

								match message {
									Ok(_) => info!("Successfully created a thread."),
									Err(_) => error!("Failed to created a thread.")
								}
							//}

							return Ok(());
						}

						http
						.create_reaction(message.channel_id, message.id, &failed)
						.exec()
						.await.unwrap();					
					}
				}
			}
		}
        _ => {}
    }

    Ok(())
}