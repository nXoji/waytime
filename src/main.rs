mod watcher;

use chrono::Local;
use watcher::WindowWatcher;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut watcher = WindowWatcher::new().await?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                let _ = watcher.unload().await;
                break;
            }
            Some(event) = watcher.next_event() => {
                let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
                println!("[{timestamp}] App: {} | Title: {}", event.app_id, event.title);
            }
        }
    }

    Ok(())
}
