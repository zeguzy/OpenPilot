use std::time::Duration;

use crossterm::event::{self, Event as CtEvent, KeyEvent};

use crate::Notification;

pub enum AppEvent {
    Key(KeyEvent),
    Notification(Notification),
    Tick,
}

pub fn poll_event(
    notif_rx: &mut tokio::sync::mpsc::UnboundedReceiver<Notification>,
) -> Option<AppEvent> {
    if event::poll(Duration::from_millis(50)).ok()? {
        if let Ok(CtEvent::Key(k)) = event::read() {
            return Some(AppEvent::Key(k));
        }
    }
    match notif_rx.try_recv() {
        Ok(n) => Some(AppEvent::Notification(n)),
        Err(_) => Some(AppEvent::Tick),
    }
}
