//! The TestBackend harness the integration tests read a frame through.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::model::App;

pub fn render_to_string(app: &App, width: u16, height: u16) -> String {
    render_to_string_with_state(app, width, height).0
}

/// Also returns the post-render state, whose click geometry tests hit-test.
pub fn render_to_string_with_state(app: &App, width: u16, height: u16) -> (String, App) {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let mut owned = app.clone();
    {
        let target = &mut owned;
        terminal
            .draw(|frame| {
                super::render(frame, target);
            })
            .expect("render");
    }
    (
        crate::common::ui::buffer_to_string(terminal.backend().buffer()),
        owned,
    )
}
