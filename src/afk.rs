use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use zbus::export::ordered_stream::OrderedStreamExt;
use zbus::{proxy, Connection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfkEvent {
    Pause,
    Resume,
}

#[proxy(
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver",
    interface = "org.freedesktop.ScreenSaver"
)]
trait ScreenSaver {
    #[zbus(signal)]
    fn active_changed(&self, active: bool) -> zbus::Result<()>;
}

#[proxy(
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    interface = "org.freedesktop.login1.Manager"
)]
trait Login1Manager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

pub struct AfkWatcher {
    rx: UnboundedReceiver<AfkEvent>,
}

impl AfkWatcher {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (raw_tx, mut raw_rx) = unbounded_channel::<AfkEvent>();
        let (tx, rx) = unbounded_channel::<AfkEvent>();

        tokio::spawn(async move {
            let mut current_state: Option<AfkEvent> = None;
            while let Some(event) = raw_rx.recv().await {
                if current_state != Some(event) {
                    current_state = Some(event);
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });

        let session_conn = Connection::session().await?;
        let screensaver = ScreenSaverProxy::new(&session_conn).await?;
        let mut screensaver_stream = screensaver.receive_active_changed().await?;

        let tx_screensaver = raw_tx.clone();
        tokio::spawn(async move {
            while let Some(signal) = screensaver_stream.next().await {
                if let Ok(args) = signal.args() {
                    let event = if *args.active() {
                        AfkEvent::Pause
                    } else {
                        AfkEvent::Resume
                    };
                    if tx_screensaver.send(event).is_err() {
                        break;
                    }
                }
            }
        });

        if let Ok(screensaver_alt) = ScreenSaverProxy::builder(&session_conn)
            .path("/ScreenSaver")?
            .build()
            .await
        {
            if let Ok(mut screensaver_alt_stream) = screensaver_alt.receive_active_changed().await {
                let tx_screensaver_alt = raw_tx.clone();
                tokio::spawn(async move {
                    while let Some(signal) = screensaver_alt_stream.next().await {
                        if let Ok(args) = signal.args() {
                            let event = if *args.active() {
                                AfkEvent::Pause
                            } else {
                                AfkEvent::Resume
                            };
                            if tx_screensaver_alt.send(event).is_err() {
                                break;
                            }
                        }
                    }
                });
            }
        }

        let system_conn = Connection::system().await?;
        let login1 = Login1ManagerProxy::new(&system_conn).await?;
        let mut sleep_stream = login1.receive_prepare_for_sleep().await?;

        let tx_sleep = raw_tx;
        tokio::spawn(async move {
            while let Some(signal) = sleep_stream.next().await {
                if let Ok(args) = signal.args() {
                    let event = if *args.start() {
                        AfkEvent::Pause
                    } else {
                        AfkEvent::Resume
                    };
                    if tx_sleep.send(event).is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self { rx })
    }

    pub async fn next_event(&mut self) -> Option<AfkEvent> {
        self.rx.recv().await
    }
}
