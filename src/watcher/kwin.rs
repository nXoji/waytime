use std::env::temp_dir;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;
use zbus::{Connection, connection::Builder, interface};

use super::{WindowEvent, WindowWatcher};

const SCRIPT_NAME: &str = "waytime_watcher";
const SCRIPT_CONTENT: &str = r#"
let connections = {};

function send(client) {
    if (!client) return;
    let appId = client.desktopFileName || client.resourceClass || "";
    let title = client.caption || "";
    callDBus(
        "org.waytime.Watcher",
        "/org/waytime/Watcher",
        "org.waytime.Watcher",
        "WindowChanged",
        String(appId),
        String(title)
    );
}

let handler = function(client) {
    if (!client) return;
    if (!(client.internalId in connections)) {
        connections[client.internalId] = true;
        client.captionChanged.connect(function() {
            if (client.active) {
                send(client);
            }
        });
    }
    send(client);
};

if (workspace.activeWindow) {
    handler(workspace.activeWindow);
}

if (workspace.windowActivated) {
    workspace.windowActivated.connect(handler);
} else if (workspace.clientActivated) {
    workspace.clientActivated.connect(handler);
}
"#;

struct ActiveWindowReceiver {
    tx: Arc<Mutex<Option<UnboundedSender<WindowEvent>>>>,
}

#[interface(name = "org.waytime.Watcher")]
impl ActiveWindowReceiver {
    async fn window_changed(&self, app_id: String, title: String) {
        if let Some(tx) = self.tx.lock().ok().as_deref().and_then(Option::as_ref) {
            let _ = tx.send(WindowEvent { app_id, title });
        }
    }
}

#[derive(Debug, Default)]
pub struct KwinWatcher {
    script_loaded: bool,
}

impl KwinWatcher {
    pub fn new() -> Self {
        Self {
            script_loaded: false,
        }
    }

    async fn init_kwin_script(
        &mut self,
        conn: &Connection,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let is_loaded: bool = conn
            .call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                "isScriptLoaded",
                &SCRIPT_NAME,
            )
            .await?
            .body()
            .deserialize()?;

        if is_loaded {
            let _: bool = conn
                .call_method(
                    Some("org.kde.KWin"),
                    "/Scripting",
                    Some("org.kde.kwin.Scripting"),
                    "unloadScript",
                    &SCRIPT_NAME,
                )
                .await?
                .body()
                .deserialize()?;
        }

        let script_path: PathBuf = temp_dir().join(format!("{SCRIPT_NAME}.js"));
        fs::write(&script_path, SCRIPT_CONTENT)?;

        let script_path_str = script_path
            .to_str()
            .ok_or("Temporary script path is invalid UTF-8")?;

        let script_id: i32 = conn
            .call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                "loadScript",
                &(script_path_str, SCRIPT_NAME),
            )
            .await?
            .body()
            .deserialize()?;

        self.script_loaded = true;

        let run_res = conn
            .call_method(
                Some("org.kde.KWin"),
                format!("/Scripting/Script{script_id}"),
                Some("org.kde.kwin.Script"),
                "run",
                &(),
            )
            .await;

        let _ = fs::remove_file(script_path);

        if let Err(e) = run_res {
            let _ = self.unload(conn).await;
            return Err(e.into());
        }

        Ok(())
    }

    pub async fn unload(
        &mut self,
        conn: &Connection,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.script_loaded {
            let _: bool = conn
                .call_method(
                    Some("org.kde.KWin"),
                    "/Scripting",
                    Some("org.kde.kwin.Scripting"),
                    "unloadScript",
                    &SCRIPT_NAME,
                )
                .await?
                .body()
                .deserialize()?;
            self.script_loaded = false;
        }
        Ok(())
    }
}

impl WindowWatcher for KwinWatcher {
    fn start(
        self: Box<Self>,
        event_tx: UnboundedSender<WindowEvent>,
        mut stop_rx: oneshot::Receiver<()>,
    ) -> tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>> {
        tokio::spawn(async move {
            let mut watcher = *self;
            let tx_slot = Arc::new(Mutex::new(Some(event_tx.clone())));
            let conn = Builder::session()?
                .name("org.waytime.Watcher")?
                .serve_at(
                    "/org/waytime/Watcher",
                    ActiveWindowReceiver {
                        tx: Arc::clone(&tx_slot),
                    },
                )?
                .build()
                .await?;

            if let Err(e) = watcher.init_kwin_script(&conn).await {
                let _ = watcher.unload(&conn).await;
                return Err(e);
            }

            tokio::select! {
                _ = event_tx.closed() => {}
                _ = &mut stop_rx => {}
            }

            if let Ok(mut guard) = tx_slot.lock() {
                guard.take();
            }
            drop(event_tx);

            let _ = watcher.unload(&conn).await;
            Ok(())
        })
    }
}

impl Drop for KwinWatcher {
    fn drop(&mut self) {
        if self.script_loaded {
            let _ = std::process::Command::new("qdbus6")
                .args([
                    "org.kde.KWin",
                    "/Scripting",
                    "org.kde.kwin.Scripting.unloadScript",
                    SCRIPT_NAME,
                ])
                .output();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kwin_watcher_trait_object_instantiation() {
        let watcher: Box<dyn WindowWatcher> = Box::new(KwinWatcher::new());
        drop(watcher);
    }
}
