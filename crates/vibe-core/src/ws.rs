//! WebSocket pub/sub for live status and log events.

use tokio::sync::broadcast;
use vibe_protocol::WsEvent;

const CHANNEL_CAPACITY: usize = 256;

#[derive(Clone)]
pub struct WsHub {
    tx: broadcast::Sender<WsEvent>,
}

impl WsHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, ev: WsEvent) {
        let _ = self.tx.send(ev);
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}
