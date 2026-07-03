use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::Cursor;

pub enum SoundEvent {
    Move,
    Capture,
    Draw,
}

pub struct AudioPlayer {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    move_bytes: Vec<u8>,
    capture_bytes: Vec<u8>,
    draw_bytes: Vec<u8>,
}

impl AudioPlayer {
    pub fn new() -> Option<Self> {
        let (stream, stream_handle) = OutputStream::try_default().ok()?;
        let base = crate::paths::asset_path("audio");
        let load = |name: &str| std::fs::read(base.join(name)).unwrap_or_default();
        Some(Self {
            _stream: stream,
            stream_handle,
            move_bytes: load("Move.mp3"),
            capture_bytes: load("Capture.mp3"),
            draw_bytes: load("Draw.mp3"),
        })
    }

    pub fn play(&self, event: SoundEvent) {
        let bytes: &[u8] = match event {
            SoundEvent::Move => &self.move_bytes,
            SoundEvent::Capture => &self.capture_bytes,
            SoundEvent::Draw => &self.draw_bytes,
        };

        if bytes.is_empty() {
            return;
        }

        let Ok(decoder) = Decoder::new(Cursor::new(bytes.to_vec())) else {
            return;
        };

        let Ok(sink) = Sink::try_new(&self.stream_handle) else {
            return;
        };

        sink.append(decoder);
        sink.detach();
    }
}
