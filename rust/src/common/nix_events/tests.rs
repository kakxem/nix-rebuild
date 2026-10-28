//! What a line of Nix output becomes: its summary, its kind and its level.

use super::*;

#[test]
fn copying_output_is_normalized_but_original_is_retained() {
    let line = "copying 'github:nixos/nixpkgs/e554fab?narHash=sha256-x/pkgs/pkgs-lib/formats/json2x' to the store";
    let event = NixEvent::from_output(line);

    assert_eq!(event.summary(), "copying  nixpkgs/…/formats/json2x → store");
    assert_eq!(event.raw(), line);
}

#[test]
fn evaluating_installable_is_compacted_for_the_log_view() {
    let event = NixEvent::from_output(
        "evaluating derivation 'git+file:///home/user/.config/nixos-dotfiles#homeConfigurations.\"sample\".config.news.json.output'",
    );
    assert_eq!(
        event.summary(),
        "evaluating  homeConfigurations.sample.config.news.json.output"
    );
}

#[test]
fn logs_are_grouped_into_scannable_semantic_kinds() {
    assert_eq!(
        NixEvent::from_output("evaluating derivation 'git+file:///repo#home'").kind(),
        LogKind::Evaluation
    );
    assert_eq!(
        NixEvent::from_output("copying path '/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo' from 'https://cache.nixos.org'...").kind(),
        LogKind::Download
    );
    assert_eq!(
        NixEvent::from_output("building '/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-demo.drv'...")
            .kind(),
        LogKind::Build
    );
    assert_eq!(
        NixEvent::from_output("Activating Home Manager configuration").kind(),
        LogKind::Activation
    );
    assert_eq!(
        NixEvent::from_output("cleanup: collecting unreachable store paths").kind(),
        LogKind::Cleanup
    );
    assert_eq!(
        NixEvent::from_output("warning: Git tree is dirty").kind(),
        LogKind::Warning
    );
}

#[test]
fn a_flake_input_copy_is_labelled_by_the_input_it_names() {
    // The exact lines a Home Manager switch emitted, one per input.
    let cases = [
        (
            "copying '«github:vicinaehq/vicinae/b66cc7241a33bf1b1181cbac6de4f9e00042d9cb?narHash=sha256-pYorP5iwAlMTkyutP7P3i40B/VLFO9VUD0BjcmSGsvM%3D»/' to the store",
            "copying  github:vicinaehq/vicinae → store",
        ),
        (
            "copying '«git+https://github.com/nix-community/plasma-manager?ref=refs/heads/trunk&rev=a19a2a029fa180911bd89c554dca1616e10f4c1d&shallow=1»/' to the store",
            "copying  nix-community/plasma-manager → store",
        ),
        (
            "copying '«github:epireyn/niri-flake/4de81ca0bc697b5f0a36c6585bf6bb0b1245ea0c?narHash=sha256-06aftJRRbmR7BIVl77o6KAUB6KZiYkNWJ6V6fEE3b5w%3D»/' to the store",
            "copying  github:epireyn/niri-flake → store",
        ),
        (
            "copying '«https://releases.nixos.org/nixos/unstable/nixos-26.11pre1083223.c59305bab206/nixexprs.tar.zst?narHash=sha256-VBA0QerzzmL%2B4wxgtwiCdiLwLpJg2/9xOxhgO3BRJQA%3D»/' to the store",
            "copying  nixexprs.tar.zst → store",
        ),
    ];

    for (line, expected) in cases {
        assert_eq!(
            NixEvent::from_output(line).summary(),
            expected,
            "input line: {line}"
        );
    }
}

#[test]
fn a_deep_sub_path_keeps_its_file_name_readable() {
    let line = "copying '«github:nixos/nixpkgs/151fa4e8ddfdd8dd25d945ad94ed54a13de9f6e4?narHash=sha256-Miqqk/ammqnTUxaoCyvtwoPeLbI1ksFyLxi/CazZpWY%3D»/pkgs/development/libraries/qt-6/hooks/fix-qt-builtin-paths.sh' to the store";

    assert_eq!(
        NixEvent::from_output(line).summary(),
        "copying  github:nixos/nixpkgs/…/hooks/fix-qt-builtin-paths.sh → store"
    );
}

#[test]
fn a_local_tree_copy_keeps_its_path_label() {
    let summary =
        NixEvent::from_output("copying '/home/user/.config/nixos-dotfiles/' to the store")
            .summary()
            .to_owned();

    assert_eq!(summary, "copying  …/.config/nixos-dotfiles → store");
}

#[test]
fn a_degraded_user_session_reports_as_a_warning_not_neutral_output() {
    // The block Home Manager printed before it carried on activating.
    let warnings = [
        "The user systemd session is degraded:",
        "● protonplus.service loaded failed failed ProtonPlus",
        "The service manager is degraded.",
        "Failed services: protonplus.service",
    ];
    for line in warnings {
        let event = NixEvent::from_output(line);
        assert_eq!(event.level, LogLevel::Warning, "level of: {line}");
        assert_eq!(event.kind(), LogKind::Warning, "kind of: {line}");
    }
}

#[test]
fn a_healthy_unit_row_and_the_session_legend_stay_neutral() {
    for line in [
        "● protonplus.service loaded active running ProtonPlus",
        "UNIT               LOAD   ACTIVE SUB    DESCRIPTION",
        "Legend: LOAD   → Reflects whether the unit definition was properly loaded.",
        "1 loaded units listed.",
        "Attempting to continue anyway...",
    ] {
        let event = NixEvent::from_output(line);
        assert_eq!(event.level, LogLevel::Info, "level of: {line}");
    }
}

#[test]
fn the_collectors_tally_reads_as_cleanup_output() {
    for line in [
        "847 store paths deleted, 3.9 GiB freed",
        "0 store paths deleted, 0 B freed.",
        "1 store path deleted, 4 KiB freed",
    ] {
        assert_eq!(
            NixEvent::from_output(line).kind(),
            LogKind::Cleanup,
            "kind of: {line}"
        );
    }
}
