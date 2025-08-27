use std::env;
use dotenvy::dotenv;
use asterisk_ari::{apis::{bridges::{models::BridgeType, params::AddChannelRequest}, params::Direction}, AriClient, Config, Result};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    let asterisk_url = env::var("ASTERISK_URL").expect("ASTERISK_URL is not defined!");
    let asterisk_user = env::var("ASTERISK_USER").expect("ASTERISK_USER is not defined!");
    let asterisk_password = env::var("ASTERISK_PASSWORD").expect("ASTERISK_PASSWORD is not defined!");

    let config = Config::new(asterisk_url, asterisk_user, asterisk_password);
    let mut client = AriClient::with_config(config);

    client.on_stasis_start(move | client, event | {
        async move {
            if event.data.channel.name.starts_with("UnicastRTP") {
                return Ok(());
            }

            println!("Channel opened: {}", &event.data.channel.id);

            client.channels().answer(&event.data.channel.id).await?;

            let external_media_params = asterisk_ari::apis::channels::params::ExternalMediaRequest::new("my_app", "77.110.104.14:5004", "ulaw").with_direction(Direction::Both);
            let external_media =  client.channels().external_media(external_media_params).await.unwrap();

            let bridge_params = asterisk_ari::apis::bridges::params::CreateRequest::new().with_type(BridgeType::Mixing);
            let bridge = client.bridges().create(bridge_params).await.unwrap();

            
            client.bridges().add_channel(AddChannelRequest::new(&bridge.id, &external_media.id)).await.unwrap();
            client.bridges().add_channel(AddChannelRequest::new(&bridge.id, &event.data.channel.id)).await.unwrap();

            Ok(())
        }
    });


    let mut _client = client.clone();
    let handle = tokio::spawn(async move {
        match _client.start("my_app").await {
            Ok(_) => println!("ARI client started successfully"),
            Err(e) => eprintln!("Failed to start ARI client: {}", e),
        }
    });

    tokio::signal::ctrl_c().await.unwrap();
    println!("Shutting down...");

    match handle.await {
        Ok(_) => println!("ARI client task finished successfully"),
        Err(e) => eprintln!("ARI client task panicked: {}", e),
    };

    Ok(())
}