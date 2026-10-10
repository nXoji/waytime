use std::collections::HashMap;
use zbus::zvariant::Value;

pub async fn send_notification(
    summary: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = match zbus::Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to connect to D-Bus session: {e}");
            return Err(e.into());
        }
    };

    let hints = HashMap::<&str, Value>::new();
    let actions: &[&str] = &[];

    match connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "waytime",
                0u32,
                "dialog-warning",
                summary,
                body,
                actions,
                &hints,
                5000i32,
            ),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("Failed to send desktop notification: {e}");
            Err(e.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_send_notification_runs_gracefully() {
        // In test environments D-Bus may or may not be active;
        // either Ok or Err is acceptable, but must never panic.
        let result = send_notification("Test Summary", "Test Body").await;
        let _ = result;
    }
}
