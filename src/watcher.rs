use std::env::temp_dir;
use std::fs;
use std::path::PathBuf;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use zbus::{connection::Builder, interface, Connection};

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

#[derive(Debug, Clone)]
pub struct WindowEvent {
    pub app_id: String,
    pub title: String,
}

struct ActiveWindowReceiver {
    tx: UnboundedSender<WindowEvent>,
}

#[interface(name = "org.waytime.Watcher")]
impl ActiveWindowReceiver {
    async fn window_changed(&self, app_id: String, title: String) {
        let _ = self.tx.send(WindowEvent { app_id, title });
    }
}

pub struct WindowWatcher {
    conn: Connection,
    rx: UnboundedReceiver<WindowEvent>,
    script_loaded: bool,
}

impl WindowWatcher {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = unbounded_channel();

        let conn = Builder::session()?
            .name("org.waytime.Watcher")?
            .serve_at("/org/waytime/Watcher", ActiveWindowReceiver { tx })?
            .build()
            .await?;

        let mut watcher = Self {
            conn,
            rx,
            script_loaded: false,
        };

        watcher.init_kwin_script().await?;
        Ok(watcher)
    }

    async fn init_kwin_script(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let is_loaded: bool = self
            .conn
            .call_method(
                Some("org.kde.KWin"),
                "/Scripting",
                Some("org.kde.kwin.Scripting"),
                "isScriptLoaded",
                &(SCRIPT_NAME),
            )
            .await?
            .body()
            .deserialize()?;

        if is_loaded {
            let _: bool = self
                .conn
                .call_method(
                    Some("org.kde.KWin"),
                    "/Scripting",
                    Some("org.kde.kwin.Scripting"),
                    "unloadScript",
                    &(SCRIPT_NAME),
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

        let script_id: i32 = self
            .conn
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

        self.conn
            .call_method(
                Some("org.kde.KWin"),
                format!("/Scripting/Script{script_id}"),
                Some("org.kde.kwin.Script"),
                "run",
                &(),
            )
            .await?;

        let _ = fs::remove_file(script_path);

        self.script_loaded = true;
        Ok(())
    }

    pub async fn next_event(&mut self) -> Option<WindowEvent> {
        self.rx.recv().await
    }

    pub async fn unload(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.script_loaded {
            let _: bool = self
                .conn
                .call_method(
                    Some("org.kde.KWin"),
                    "/Scripting",
                    Some("org.kde.kwin.Scripting"),
                    "unloadScript",
                    &(SCRIPT_NAME),
                )
                .await?
                .body()
                .deserialize()?;
            self.script_loaded = false;
        }
        Ok(())
    }
}

impl Drop for WindowWatcher {
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
