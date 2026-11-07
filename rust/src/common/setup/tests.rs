//! The declaration the wrapper carries, and what happens when it is missing.

use super::{HomeManager, home_config, home_manager, impure, system_config, trust_flake_config};

/// Run `body` with the declaration cleared, as a bare binary sees it.
fn without_a_declaration<T>(body: impl FnOnce() -> T) -> T {
    temp_env::with_var("NIX_REBUILD_HOME_MANAGER", None::<&str>, || {
        temp_env::with_var("NIX_REBUILD_SYSTEM_CONFIG", None::<&str>, || {
            temp_env::with_var("NIX_REBUILD_HOME_CONFIG", None::<&str>, || {
                temp_env::with_var("NIX_REBUILD_IMPURE", None::<&str>, || {
                    temp_env::with_var("NIX_REBUILD_TRUST_FLAKE_CONFIG", None::<&str>, body)
                })
            })
        })
    })
}

#[test]
fn a_machine_that_declares_nothing_is_told_what_to_declare() {
    without_a_declaration(|| {
        let kind = home_manager().expect_err("no kind is declared");
        assert!(
            kind.to_string().contains("`home.manager`"),
            "the message names the option: {kind}"
        );

        let system = system_config().expect_err("no system name is declared");
        assert!(
            system.to_string().contains("`system.config`"),
            "the message names the option: {system}"
        );

        let home = home_config().expect_err("no home name is declared");
        assert!(
            home.to_string().contains("`home.config`"),
            "the message names the option: {home}"
        );

        assert!(
            !impure(),
            "an undeclared machine evaluates its flake the way Nix already does"
        );

        assert!(
            !trust_flake_config(),
            "an undeclared machine keeps the flake's settings untrusted, as Nix does"
        );
    });
}

#[test]
fn a_machine_that_declares_an_impure_flake_is_heard() {
    for value in ["1", "true"] {
        temp_env::with_var("NIX_REBUILD_IMPURE", Some(value), || {
            assert!(impure(), "{value} evaluates the flake impurely");
        });
    }

    // A written `false` or `0` means the same as no declaration at all.
    for value in ["0", "false", ""] {
        temp_env::with_var("NIX_REBUILD_IMPURE", Some(value), || {
            assert!(!impure(), "{value:?} leaves the evaluation pure");
        });
    }
}

#[test]
fn a_machine_that_declares_the_flakes_config_trusted_is_heard() {
    for value in ["1", "true"] {
        temp_env::with_var("NIX_REBUILD_TRUST_FLAKE_CONFIG", Some(value), || {
            assert!(
                trust_flake_config(),
                "{value} accepts the flake's nixConfig"
            );
        });
    }

    for value in ["0", "false", ""] {
        temp_env::with_var("NIX_REBUILD_TRUST_FLAKE_CONFIG", Some(value), || {
            assert!(
                !trust_flake_config(),
                "{value:?} leaves the flake's settings untrusted"
            );
        });
    }
}

#[test]
fn nothing_is_guessed_from_the_environment() {
    without_a_declaration(|| {
        temp_env::with_var("USER", Some("someone"), || {
            temp_env::with_var("HOSTNAME", Some("somehost"), || {
                assert!(
                    home_config().is_err(),
                    "the user running the tool does not name its configuration"
                );
                assert!(
                    system_config().is_err(),
                    "nor does the hostname name the system configuration"
                );
            });
        });
    });
}

#[test]
fn what_the_machine_declares_is_what_the_tool_uses() {
    temp_env::with_var("NIX_REBUILD_HOME_MANAGER", Some("module"), || {
        temp_env::with_var("NIX_REBUILD_SYSTEM_CONFIG", Some("myhost"), || {
            temp_env::with_var("NIX_REBUILD_HOME_CONFIG", Some("work"), || {
                assert_eq!(home_manager().expect("a kind"), HomeManager::Module);
                assert_eq!(system_config().expect("a system name"), "myhost");
                assert_eq!(home_config().expect("a home name"), "work");
            });
        });
    });
}

#[test]
fn an_empty_value_is_not_a_declaration() {
    temp_env::with_var("NIX_REBUILD_SYSTEM_CONFIG", Some(""), || {
        assert!(system_config().is_err(), "an empty value names nothing");
    });
}

#[test]
fn an_attribute_may_be_named_alone_or_with_its_path() {
    for value in [
        "work",
        "\"work\"",
        "homeConfigurations.work",
        "homeConfigurations.\"work\"",
    ] {
        temp_env::with_var("NIX_REBUILD_HOME_CONFIG", Some(value), || {
            assert_eq!(home_config().expect("a home name"), "work", "{value}");
        });
    }

    for value in [
        "myhost",
        "nixosConfigurations.myhost",
        "nixosConfigurations.\"myhost\"",
    ] {
        temp_env::with_var("NIX_REBUILD_SYSTEM_CONFIG", Some(value), || {
            assert_eq!(system_config().expect("a system name"), "myhost", "{value}");
        });
    }
}

#[test]
fn a_kind_that_is_not_one_of_the_three_is_refused() {
    temp_env::with_var("NIX_REBUILD_HOME_MANAGER", Some("nonsense"), || {
        let refused = home_manager().expect_err("a kind the tool does not know");
        let message = refused.to_string();
        assert!(
            message.contains("standalone")
                && message.contains("module")
                && message.contains("none"),
            "the message lists the kinds it does know: {message}"
        );
    });
}

#[test]
fn the_two_kinds_that_rebuild_no_home_say_so_in_their_own_words() {
    assert_eq!(
        HomeManager::Standalone.why_home_rebuilds_nothing(),
        "home rebuilds the home configuration",
        "the standalone case has nothing to explain"
    );

    let module = HomeManager::Module.why_home_rebuilds_nothing();
    assert!(
        module.contains("`nix-rebuild system`"),
        "a module of NixOS is activated by the system scope: {module}"
    );

    let none = HomeManager::None.why_home_rebuilds_nothing();
    assert!(
        none.contains("does not use Home Manager"),
        "a machine without it says so: {none}"
    );
}
