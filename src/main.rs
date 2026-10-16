mod actions;
mod concat;
mod keys;

use openaction::*;
use simplelog::{Config, LevelFilter, WriteLogger};

#[tokio::main]
async fn main() -> OpenActionResult<()> {
    // Stream Deck starts the plugin with its .sdPlugin folder as the cwd.
    if let Ok(file) =
        std::fs::create_dir_all("logs").and_then(|()| std::fs::File::create("logs/plugin.log"))
    {
        let _ = WriteLogger::init(LevelFilter::Info, Config::default(), file);
    }

    register_action(actions::CommandKey).await;
    register_action(actions::JogDial).await;
    register_action(actions::ZoomDial).await;
    register_action(actions::StatusKey).await;
    register_action(actions::RenderAction).await;

    concat::client();
    tokio::spawn(actions::StatusKey::watch());
    tokio::spawn(actions::RenderAction::watch());

    run(std::env::args().collect()).await
}
