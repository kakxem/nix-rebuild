//! The settings file parses leniently, documents itself and round-trips.

use crate::common::cleanup_policy::CleanupPolicy;

use super::{LeaveScreen, Settings};

#[test]
fn the_settings_file_is_read_leniently() {
    let settings = Settings::parse(
        "\
# a comment

cleanup = \"never\"
keep_generations = 3
checkout = \"/home/user/.config/nixos-dotfiles#sample\"
show_downloads = false
raw_logs = yes
nonsense
unknown_key = 4
show_logs =
",
    );

    assert_eq!(settings.cleanup, CleanupPolicy::Never);
    assert_eq!(settings.keep_generations, 3);
    assert_eq!(
        settings.checkout.as_deref(),
        Some("/home/user/.config/nixos-dotfiles#sample"),
        "a quoted value keeps its hash"
    );
    assert!(!settings.show_downloads);
    assert!(settings.raw_logs);
    assert!(settings.show_logs, "an empty value keeps its default");
    assert!(settings.show_status, "an unknown key is ignored");
    assert_eq!(
        settings.leave,
        LeaveScreen::Clean,
        "and so is a value that is not one of the two"
    );
}

#[test]
fn the_settings_say_what_the_frame_leaves_on_the_terminal() {
    // Accepted words: `frame`, `keep` (the older one) and `clean`.
    assert_eq!(
        Settings::parse("leave = \"frame\"").leave,
        LeaveScreen::Frame
    );
    assert_eq!(
        Settings::parse("leave = \"clean\"").leave,
        LeaveScreen::Clean
    );
    assert_eq!(
        Settings::parse("leave = \"nonsense\"").leave,
        Settings::default().leave,
        "an unreadable value keeps the default"
    );

    let rendered = Settings::default().render();
    assert!(
        rendered.contains("leave = \"clean\""),
        "the default is written out:\n{rendered}"
    );
    assert!(
        rendered.contains("never reaches the scrollback"),
        "and says why a resize cannot dirty the terminal:\n{rendered}"
    );
    assert!(
        rendered.contains("`clean` leaves"),
        "and explains what the default keeps:\n{rendered}"
    );
}

#[test]
fn the_settings_file_round_trips() {
    let settings = Settings {
        cleanup: CleanupPolicy::Always,
        keep_generations: 2,
        checkout: Some("/tmp/checkout".into()),
        leave: LeaveScreen::Frame,
        show_cleanup_progress: false,
        ..Default::default()
    };

    assert_eq!(Settings::parse(&settings.render()), settings);
    assert_eq!(
        Settings::parse(&Settings::default().render()),
        Settings::default()
    );
}

#[test]
fn a_saved_log_filter_is_written_and_read_back() {
    let mut settings = super::Settings::default();
    assert_eq!(
        settings.log_filter_query, None,
        "no filter is saved at first"
    );
    assert!(
        settings.render().contains("# log_filter_query"),
        "and the file says where one would go"
    );

    settings.log_filter_query = Some("error".into());
    settings.log_filter_kinds = Some("errors,warnings".into());
    let text = settings.render();
    assert!(
        text.contains("log_filter_query = \"error\""),
        "the query is written plainly:\n{text}"
    );
    assert!(text.contains("log_filter_kinds = \"errors,warnings\""));

    let read = super::Settings::parse(&text);
    assert_eq!(read.log_filter_query.as_deref(), Some("error"));
    assert_eq!(read.log_filter_kinds.as_deref(), Some("errors,warnings"));
}

#[test]
fn a_filter_with_no_kinds_saved_reads_as_no_kinds() {
    let read = super::Settings::parse("log_filter_kinds = \"\"\n");
    assert_eq!(
        read.log_filter_kinds.as_deref(),
        Some(""),
        "an empty list is a saved answer, not a missing one"
    );
}
