//! Terminal primitives: redraw clock, inline viewport, raw-mode and mouse guards,
//! reader threads and the writers the frame and plain commands share.

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use crossbeam_channel::Sender;
use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, MouseEvent,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

pub const REDRAW_INTERVAL: Duration = Duration::from_millis(50);

pub fn redraw_due(last: Instant, now: Instant, force: bool) -> bool {
    force || now.saturating_duration_since(last) >= REDRAW_INTERVAL
}

/// Write text to stdout, treating a broken pipe as a normal end (`runs --list | head`);
/// `print!` would panic on the closed pipe.
pub fn write_stdout(text: &str) -> anyhow::Result<()> {
    let mut stdout = io::stdout();
    match stdout.write_all(text.as_bytes()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[derive(Debug)]
pub enum RuntimeEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Signal(i32),
    Output(String),
    /// A stderr line: shown with the output, kept apart as the last word on a failure.
    Stderr(String),
    Json(String),
    CleanupDone(bool),
}

/// The frame's terminal: the alternate screen, raw mode and mouse capture, released
/// together so a panic cannot leave the shell on a screen this process drew.
pub struct ScreenGuard;

impl ScreenGuard {
    pub fn enter() -> anyhow::Result<Self> {
        crossterm::execute!(
            io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )
        .context("enter the alternate screen")?;
        if let Err(error) = enable_raw_mode() {
            let _ = crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen);
            return Err(error).context("enable terminal raw mode");
        }
        Ok(Self)
    }
}

/// Releases mouse capture so text can be selected; F2 toggles it.
#[derive(Default)]
pub struct SelectGuard {
    active: bool,
}

impl SelectGuard {
    pub fn toggle(&mut self) {
        let select = !self.active;
        self.active = select;
        let result = if select {
            crossterm::execute!(io::stdout(), crossterm::event::DisableMouseCapture)
        } else {
            crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture)
        };
        let _ = result;
    }
}

impl Drop for SelectGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture);
        }
    }
}

impl Drop for ScreenGuard {
    fn drop(&mut self) {
        restore_screen();
    }
}

/// Hand the terminal back: mouse, cursor, raw mode, and the shell's own screen, which
/// comes back exactly as it was because the frame was never drawn on it.
fn restore_screen() {
    let _ = crossterm::execute!(
        io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show,
        crossterm::terminal::LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}

/// A panic inside the frame would print its message on the alternate screen and be wiped
/// with it, so the terminal is given back before the hook writes anything.
pub fn restore_on_panic() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_screen();
            previous(info);
        }));
    });
}

/// F2 releases the mouse for text selection; checked before any overlay, so it works in every dialog.
pub fn is_select_toggle(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press && key.code == KeyCode::F(2)
}

/// Read a child's stream line by line, one event per line, wrapped by the caller's
/// `event` constructor so a line keeps the stream it came from.
pub fn spawn_reader<R: io::Read + Send + 'static>(
    reader: R,
    tx: Sender<RuntimeEvent>,
    event: fn(String) -> RuntimeEvent,
) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let _ = tx.send(event(line));
        }
    });
}

pub fn spawn_signals(tx: Sender<RuntimeEvent>) {
    thread::spawn(move || {
        let Ok(mut signals) = Signals::new([SIGINT, SIGTERM, SIGHUP]) else {
            return;
        };
        for signal in signals.forever() {
            if tx.send(RuntimeEvent::Signal(signal)).is_err() {
                break;
            }
        }
    });
}

pub fn spawn_input(tx: Sender<RuntimeEvent>, stop: Arc<AtomicBool>) {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            if !event::poll(Duration::from_millis(100)).unwrap_or(false) {
                continue;
            }
            match event::read() {
                Ok(CrosstermEvent::Key(key)) => {
                    let _ = tx.send(RuntimeEvent::Key(key));
                }
                Ok(CrosstermEvent::Mouse(mouse)) => {
                    let _ = tx.send(RuntimeEvent::Mouse(mouse));
                }
                _ => {}
            }
        }
    });
}

pub fn spawn_log_tail(path: PathBuf, tx: Sender<RuntimeEvent>, stop: Arc<AtomicBool>) {
    thread::spawn(move || {
        let mut position = 0;
        while !stop.load(Ordering::Relaxed) {
            if let Ok(mut file) = OpenOptions::new().read(true).open(&path) {
                let _ = file.seek(SeekFrom::Start(position));
                let mut reader = BufReader::new(file);
                let mut line = String::new();
                loop {
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(bytes) => {
                            position += bytes as u64;
                            let trimmed = line.trim();
                            if !trimmed.is_empty() {
                                let _ = tx.send(RuntimeEvent::Json(trimmed.into()));
                            }
                            line.clear();
                        }
                        Err(_) => break,
                    }
                }
            }
            thread::sleep(Duration::from_millis(80));
        }
    });
}

pub fn event_log_path() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path = PathBuf::from(runtime);
        if path.is_dir() {
            return path.join(format!("nix-rebuild-{}.jsonl", std::process::id()));
        }
    }
    std::env::temp_dir().join(format!(
        "nix-rebuild-{}-{}.jsonl",
        nix::unistd::geteuid(),
        std::process::id()
    ))
}

pub fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(code.clamp(0, 255) as u8)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn redraw_scheduler_is_rate_limited_but_final_frames_are_forced() {
        let last = std::time::Instant::now();
        assert!(!super::redraw_due(
            last,
            last + Duration::from_millis(20),
            false,
        ));
        assert!(super::redraw_due(
            last,
            last + Duration::from_millis(50),
            false,
        ));
        assert!(super::redraw_due(last, last, true));
    }

    #[test]
    fn only_a_pressed_f2_releases_the_mouse() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert!(
            super::is_select_toggle(&press(KeyCode::F(2))),
            "F2 releases the mouse so text can be selected"
        );
        for key in [press(KeyCode::F(3)), press(KeyCode::Char('2'))] {
            assert!(!super::is_select_toggle(&key), "{key:?} is not the toggle");
        }
        let mut release = press(KeyCode::F(2));
        release.kind = KeyEventKind::Release;
        assert!(
            !super::is_select_toggle(&release),
            "a release is not the press"
        );
    }
}
