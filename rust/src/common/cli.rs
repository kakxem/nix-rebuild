use std::env;
use std::fmt;

use clap::builder::PossibleValue;
use clap::{Parser, ValueEnum};

use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::scope::Scope;

/// What a run does: an activation, the update sequence, or a verb that only reads the state
/// a run left behind (`runs`, `locks`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    System,
    Home,
    Update,
    /// Not a rebuild of its own: opens, lists or deletes recorded runs.
    Runs,
    /// Puts a kept lock file back on the checkout and rebuilds with it.
    Locks,
    /// Not a rebuild either: the settings card in a frame of its own.
    Settings,
}

impl ValueEnum for Target {
    fn value_variants<'a>() -> &'a [Self] {
        &[
            Self::System,
            Self::Home,
            Self::Update,
            Self::Runs,
            Self::Locks,
            Self::Settings,
        ]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::System => PossibleValue::new("system"),
            Self::Home => PossibleValue::new("home"),
            Self::Update => PossibleValue::new("update"),
            // The old name still opens the selector.
            Self::Runs => PossibleValue::new("runs").alias("resume"),
            Self::Locks => PossibleValue::new("locks"),
            Self::Settings => PossibleValue::new("settings"),
        })
    }
}

impl Target {
    pub fn scope(self) -> Option<Scope> {
        match self {
            Self::System => Some(Scope::System),
            Self::Home => Some(Scope::Home),
            Self::Update | Self::Runs | Self::Locks | Self::Settings => None,
        }
    }

    pub fn opens_a_run(self) -> bool {
        self == Self::Runs
    }

    pub fn opens_locks(self) -> bool {
        self == Self::Locks
    }

    pub fn edits_settings(self) -> bool {
        self == Self::Settings
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::System => "system",
            Self::Home => "home",
            Self::Update => "update",
            Self::Runs => "runs",
            Self::Locks => "locks",
            Self::Settings => "settings",
        })
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "nix-rebuild",
    version,
    about = "Interactive NixOS and Home Manager rebuild interface",
    override_usage = "nix-rebuild <system|home|update> [OPTIONS]\n       nix-rebuild runs [<ID>] [--list] [--logs] [--delete | --delete-all]\n       nix-rebuild locks [<ID>] [--list]\n       nix-rebuild settings"
)]
pub struct Cli {
    /// What to run. Not needed to open a recorded run.
    #[arg(value_enum, value_name = "TARGET")]
    pub target: Option<Target>,

    /// The recorded run `runs` opens, or the kept lock `locks` puts back: `last` for the
    /// newest, otherwise an id from the list. Without it, both verbs ask which one.
    #[arg(value_name = "ID")]
    pub id: Option<String>,

    #[arg(long, help = "Register the NixOS generation for the next boot")]
    pub boot: bool,

    #[arg(
        short = 'y',
        long,
        conflicts_with = "no_clean",
        help = "Run cleanup without asking"
    )]
    pub yes: bool,

    #[arg(
        long,
        conflicts_with = "yes",
        help = "Skip generation pruning and garbage collection"
    )]
    pub no_clean: bool,

    #[arg(
        long,
        help = "The flake checkout to rebuild; defaults to the one in the settings file"
    )]
    pub flake: Option<String>,

    #[arg(
        long,
        help = "Accept what the flake's nixConfig sets (substituters, keys); otherwise only the declared trust counts"
    )]
    pub accept_flake_config: bool,

    #[arg(
        long,
        conflicts_with_all = ["id", "logs"],
        help = "Print the recorded runs or the kept locks, with their ids, instead of asking which to open"
    )]
    pub list: bool,

    #[arg(
        long,
        conflicts_with = "list",
        help = "Print a recorded run's log instead of drawing it, for a pipe; with no id, the newest run"
    )]
    pub logs: bool,

    #[arg(
        long,
        conflicts_with_all = ["list", "logs", "delete_all"],
        help = "Delete the recorded run named by ID"
    )]
    pub delete: bool,

    #[arg(
        long,
        conflicts_with_all = ["list", "logs", "delete", "id"],
        help = "Delete every recorded run"
    )]
    pub delete_all: bool,
}

impl Cli {
    pub fn opens_a_run(&self) -> bool {
        self.target.is_some_and(Target::opens_a_run)
    }

    pub fn opens_locks(&self) -> bool {
        self.target.is_some_and(Target::opens_locks)
    }

    pub fn check_run_arguments(&self) -> Result<(), String> {
        if self.target.is_some_and(Target::edits_settings) {
            // Settings only edits the file, so every flag that changes a run is a mistake.
            for (given, flag) in [
                (self.boot, "--boot"),
                (self.no_clean, "--no-clean"),
                (self.yes, "--yes"),
                (self.flake.is_some(), "--flake"),
                (self.accept_flake_config, "--accept-flake-config"),
                (self.list, "--list"),
                (self.logs, "--logs"),
                (self.delete, "--delete"),
                (self.delete_all, "--delete-all"),
                (self.id.is_some(), "an id"),
            ] {
                if given {
                    return Err(format!(
                        "{flag} is not valid with `nix-rebuild settings`: nothing is rebuilt, the file is edited"
                    ));
                }
            }
            return Ok(());
        }
        if self.opens_a_run() {
            // Nothing here rebuilds, so a flag that only changes a rebuild is a mistake.
            for (given, flag) in [
                (self.boot, "--boot"),
                (self.no_clean, "--no-clean"),
                (self.flake.is_some(), "--flake"),
                (self.accept_flake_config, "--accept-flake-config"),
            ] {
                if given {
                    return Err(format!(
                        "{flag} is not valid with `nix-rebuild runs`: a recorded run is never rebuilt"
                    ));
                }
            }
            // `--yes` answers a question: a rebuild's cleanup, or a deletion's confirmation.
            if self.yes && !(self.delete || self.delete_all) {
                return Err(
                    "--yes is only valid with a deletion here: `runs <ID> --delete --yes`".into(),
                );
            }
            if self.delete && self.id.is_none() {
                return Err(
                    "name the run to delete: `nix-rebuild runs <ID> --delete`, or pass --delete-all"
                        .into(),
                );
            }
            return Ok(());
        }
        if self.target.is_some_and(Target::opens_locks) {
            // A rollback rebuilds, so the flags that change a rebuild stay; reading and
            // deleting belong to the journal.
            for (given, flag) in [
                (self.logs, "--logs"),
                (self.delete, "--delete"),
                (self.delete_all, "--delete-all"),
            ] {
                if given {
                    return Err(format!(
                        "{flag} is not valid with `nix-rebuild locks`: a kept lock is rebuilt with, not read or deleted"
                    ));
                }
            }
            return Ok(());
        }
        if self.delete || self.delete_all {
            return Err("a deletion belongs to `nix-rebuild runs`".into());
        }
        if self.id.is_some() {
            return Err("a run id is only valid with `nix-rebuild runs`".into());
        }
        if self.logs {
            return Err("--logs is only valid with `nix-rebuild runs`".into());
        }
        if self.list {
            return Err("--list is only valid with `nix-rebuild runs`".into());
        }
        Ok(())
    }

    /// An explicit flag beats the settings file, which is only a default.
    pub fn cleanup_policy(&self, configured: CleanupPolicy) -> CleanupPolicy {
        match (self.yes, self.no_clean) {
            (true, _) => CleanupPolicy::Always,
            (_, true) => CleanupPolicy::Never,
            _ => configured,
        }
    }

    /// Precedence: `--flake`, then the machine's declaration (`FLAKE_PATH`), then the
    /// settings file. The machine declares its checkout, so a flag beats it and the
    /// file is only what a machine without a declaration falls back to.
    pub fn checkout(&self, configured: Option<&str>) -> Option<String> {
        self.flake
            .clone()
            .or_else(|| env::var("FLAKE_PATH").ok())
            .or_else(|| configured.map(str::to_owned))
            .filter(|checkout| !checkout.is_empty())
    }

    /// The flag turns the trust on for one run; the declaration sets the machine's
    /// standing answer. Either on means on: the flag has no way to un-declare.
    pub fn trust_flake_config(&self) -> bool {
        self.accept_flake_config || crate::common::setup::trust_flake_config()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{CleanupPolicy, Scope};

    #[test]
    fn cleanup_flags_are_mutually_exclusive() {
        use crate::common::cli::Cli;
        use clap::Parser;

        let rejected = Cli::try_parse_from(["nix-rebuild", "home", "--yes", "--no-clean"]);
        assert!(
            rejected.is_err(),
            "--yes --no-clean is contradictory and must be rejected",
        );

        assert!(Cli::try_parse_from(["nix-rebuild", "home", "--yes"]).is_ok());
        assert!(Cli::try_parse_from(["nix-rebuild", "home", "--no-clean"]).is_ok());
        assert!(Cli::try_parse_from(["nix-rebuild", "home"]).is_ok());
    }

    #[test]
    fn runs_is_a_verb_that_names_a_run_without_rebuilding() {
        use crate::common::cli::{Cli, Target};
        use clap::Parser;

        let picker = Cli::parse_from(["nix-rebuild", "runs"]);
        assert_eq!(picker.target, Some(Target::Runs));
        assert!(picker.opens_a_run(), "it never rebuilds");
        assert_eq!(picker.id, None, "no id means ask which one");
        assert_eq!(Target::Runs.scope(), None, "there is no scope to rebuild");

        // The old name is an alias and opens the same selector.
        let alias = Cli::parse_from(["nix-rebuild", "resume"]);
        assert_eq!(alias.target, Some(Target::Runs));
        assert!(alias.opens_a_run());

        let named = Cli::parse_from(["nix-rebuild", "runs", "last"]);
        assert_eq!(named.id.as_deref(), Some("last"));

        // The flags can sit on either side of the id the verb takes.
        let before = Cli::parse_from(["nix-rebuild", "runs", "--logs", "last"]);
        assert_eq!(before.id.as_deref(), Some("last"));
        assert!(before.logs);

        assert!(!Cli::parse_from(["nix-rebuild", "home"]).opens_a_run());
        assert!(!Cli::parse_from(["nix-rebuild", "home", "--list"]).opens_a_run());
    }

    #[test]
    fn locks_is_a_verb_that_puts_a_kept_lock_back_and_rebuilds() {
        use crate::common::cli::{Cli, Target};
        use clap::Parser;

        let picker = Cli::parse_from(["nix-rebuild", "locks"]);
        assert_eq!(picker.target, Some(Target::Locks));
        assert!(picker.opens_locks(), "it puts a lock back and rebuilds");
        assert_eq!(
            Target::Locks.scope(),
            None,
            "the plan decides the scopes, not the verb"
        );
        assert_eq!(picker.id, None, "no id means ask which one");

        let named = Cli::parse_from(["nix-rebuild", "locks", "last"]);
        assert_eq!(named.id.as_deref(), Some("last"));
        assert!(named.check_run_arguments().is_ok());

        // It rebuilds, so the flags that change a rebuild are its own.
        for args in [
            vec!["nix-rebuild", "locks", "--boot"],
            vec!["nix-rebuild", "locks", "--yes"],
            vec!["nix-rebuild", "locks", "--no-clean"],
            vec!["nix-rebuild", "locks", "--flake", "/repo"],
            vec!["nix-rebuild", "locks", "--list"],
        ] {
            let cli = Cli::parse_from(args.iter().copied());
            assert!(
                cli.check_run_arguments().is_ok(),
                "{args:?} changes a rebuild, so a rollback takes it"
            );
        }

        // And nothing of the journal's, which reads and deletes recorded runs.
        for args in [
            vec!["nix-rebuild", "locks", "--logs"],
            vec!["nix-rebuild", "locks", "an-id", "--delete"],
            vec!["nix-rebuild", "locks", "--delete-all"],
        ] {
            let cli = Cli::parse_from(args.iter().copied());
            assert!(
                cli.check_run_arguments().is_err(),
                "{args:?} belongs to `nix-rebuild runs`"
            );
        }

        // A listing is the whole answer, so it takes no id.
        assert!(Cli::try_parse_from(["nix-rebuild", "locks", "--list", "last"]).is_err());
    }

    #[test]
    fn the_listing_print_flag_does_not_take_a_run() {
        use crate::common::cli::Cli;
        use clap::Parser;

        let listing = Cli::parse_from(["nix-rebuild", "runs", "--list"]);
        assert!(listing.list);
        assert_eq!(
            listing.id, None,
            "the whole list is the answer, not one run"
        );

        // A run id or `--logs` beside `--list` is two answers to one question.
        assert!(Cli::try_parse_from(["nix-rebuild", "runs", "--list", "last"]).is_err());
        assert!(Cli::try_parse_from(["nix-rebuild", "runs", "--list", "--logs"]).is_err());
    }

    #[test]
    fn run_only_arguments_are_rejected_elsewhere() {
        use crate::common::cli::Cli;
        use clap::Parser;

        let id_beside_a_target = Cli::parse_from(["nix-rebuild", "home", "last"]);
        assert!(
            id_beside_a_target.check_run_arguments().is_err(),
            "an id is not a scope to rebuild",
        );

        let logs_beside_a_target = Cli::parse_from(["nix-rebuild", "update", "--logs"]);
        assert!(
            logs_beside_a_target.check_run_arguments().is_err(),
            "--logs reads a recorded run, so it needs one",
        );

        let list_beside_a_target = Cli::parse_from(["nix-rebuild", "update", "--list"]);
        assert!(
            list_beside_a_target.check_run_arguments().is_err(),
            "--list reads the recorded runs, so it needs the verb that opens them",
        );

        assert!(
            Cli::parse_from(["nix-rebuild", "runs", "--list"])
                .check_run_arguments()
                .is_ok()
        );
        assert!(
            Cli::parse_from(["nix-rebuild", "runs", "--logs"])
                .check_run_arguments()
                .is_ok()
        );
        assert!(
            Cli::parse_from(["nix-rebuild", "runs", "last", "--logs"])
                .check_run_arguments()
                .is_ok()
        );

        assert!(
            Cli::parse_from(["nix-rebuild", "home"])
                .check_run_arguments()
                .is_ok()
        );
    }

    #[test]
    fn a_cleanup_flag_beats_what_the_settings_remember() {
        use crate::common::cli::Cli;
        use clap::Parser;

        let plain = Cli::parse_from(["nix-rebuild", "home"]);
        assert_eq!(
            plain.cleanup_policy(CleanupPolicy::Never),
            CleanupPolicy::Never,
            "without a flag the file decides"
        );

        let yes = Cli::parse_from(["nix-rebuild", "home", "--yes"]);
        assert_eq!(
            yes.cleanup_policy(CleanupPolicy::Never),
            CleanupPolicy::Always
        );

        let off = Cli::parse_from(["nix-rebuild", "home", "--no-clean"]);
        assert_eq!(
            off.cleanup_policy(CleanupPolicy::Always),
            CleanupPolicy::Never
        );
    }

    #[test]
    fn the_checkout_comes_from_the_flag_then_the_declaration_then_the_file() {
        use crate::common::cli::Cli;
        use clap::Parser;

        let flag = Cli::parse_from(["nix-rebuild", "home", "--flake", "/tmp/flag"]);
        let declared = Cli::parse_from(["nix-rebuild", "home"]);

        temp_env::with_var("FLAKE_PATH", Some("/tmp/declared"), || {
            assert_eq!(
                flag.checkout(Some("/tmp/file")).as_deref(),
                Some("/tmp/flag"),
                "the flag wins"
            );
            assert_eq!(
                declared.checkout(Some("/tmp/file")).as_deref(),
                Some("/tmp/declared"),
                "the machine's declaration beats the file, which is only a fallback"
            );
        });

        temp_env::with_var("FLAKE_PATH", None::<&str>, || {
            assert_eq!(
                declared.checkout(Some("/tmp/file")).as_deref(),
                Some("/tmp/file"),
                "with no declaration the file decides"
            );
            assert_eq!(
                declared.checkout(None),
                None,
                "and with neither, there is no checkout to guess at"
            );
        });
    }

    #[test]
    fn settings_is_a_verb_of_its_own() {
        use crate::common::cli::{Cli, Target};
        use clap::Parser;

        let cli = Cli::try_parse_from(["nix-rebuild", "settings"]).expect("settings parses");

        assert_eq!(cli.target, Some(Target::Settings));
        assert!(
            cli.target.and_then(Target::scope).is_none(),
            "nothing is rebuilt"
        );
        assert!(
            cli.check_run_arguments().is_ok(),
            "a bare `settings` is valid"
        );
    }

    #[test]
    fn the_trust_flag_is_a_rebuild_flag_with_its_own_verb_rules() {
        use crate::common::cli::Cli;
        use clap::Parser;

        // The flag only turns things on, so a machine's declaration already
        // covering it is not asserted here: `setup/tests.rs` reads that side.
        assert!(
            Cli::parse_from(["nix-rebuild", "home", "--accept-flake-config"]).trust_flake_config(),
            "the flag turns the trust on for one run"
        );
        assert!(
            !Cli::parse_from(["nix-rebuild", "home"]).accept_flake_config,
            "without it, only the machine's declaration decides"
        );

        let locks = Cli::parse_from(["nix-rebuild", "locks", "--accept-flake-config"]);
        assert!(
            locks.check_run_arguments().is_ok(),
            "a rollback rebuilds, so the lock verb takes the flag"
        );

        for args in [
            vec!["nix-rebuild", "runs", "--accept-flake-config"],
            vec!["nix-rebuild", "settings", "--accept-flake-config"],
        ] {
            let cli = Cli::parse_from(args.iter().copied());
            assert!(
                cli.check_run_arguments().is_err(),
                "{args:?} rebuilds nothing, so there is no trust to give"
            );
        }
    }

    #[test]
    fn settings_refuses_every_flag_that_changes_a_run() {
        use crate::common::cli::Cli;
        use clap::Parser;

        for arguments in [
            vec!["nix-rebuild", "settings", "--flake", "/repo"],
            vec!["nix-rebuild", "settings", "--boot"],
            vec!["nix-rebuild", "settings", "--no-clean"],
            vec!["nix-rebuild", "settings", "--yes"],
            vec!["nix-rebuild", "settings", "--list"],
            vec!["nix-rebuild", "settings", "--logs"],
            vec!["nix-rebuild", "settings", "an-id"],
            vec!["nix-rebuild", "settings", "--delete", "an-id"],
        ] {
            let cli = Cli::try_parse_from(&arguments)
                .unwrap_or_else(|error| panic!("{arguments:?} parses: {error}"));

            assert!(
                cli.check_run_arguments().is_err(),
                "{arguments:?} must be refused: settings only edits the file"
            );
        }
    }

    #[test]
    fn the_fish_completion_names_every_target_and_flag() {
        use clap::CommandFactory;

        let completion = include_str!("../../../completions/nix-rebuild.fish");
        let command = super::Cli::command();

        let target = command
            .get_arguments()
            .find(|argument| argument.get_id() == "target")
            .expect("the target argument");
        for value in target.get_possible_values() {
            assert!(
                completion.contains(&format!("-a {} ", value.get_name())),
                "the completion offers `{}`",
                value.get_name()
            );
        }
        for argument in command.get_arguments() {
            if let Some(long) = argument.get_long() {
                assert!(
                    completion.contains(&format!("-l {long}")),
                    "the completion offers `--{long}`"
                );
            }
        }
    }

    #[test]
    fn a_deletion_is_a_runs_verb_that_names_what_it_removes() {
        use crate::common::cli::Cli;
        use clap::Parser;

        let one = Cli::parse_from(["nix-rebuild", "runs", "abc", "--delete"]);
        assert!(one.delete && !one.delete_all);
        assert_eq!(one.id.as_deref(), Some("abc"));
        assert!(one.check_run_arguments().is_ok());

        let all = Cli::parse_from(["nix-rebuild", "runs", "--delete-all"]);
        assert!(all.delete_all);
        assert!(all.check_run_arguments().is_ok());

        // One answer at a time: all is not one, and reading is not deleting.
        assert!(Cli::try_parse_from(["nix-rebuild", "runs", "--delete-all", "abc"]).is_err());
        assert!(Cli::try_parse_from(["nix-rebuild", "runs", "abc", "--delete", "--logs"]).is_err());
        assert!(Cli::try_parse_from(["nix-rebuild", "runs", "--delete-all", "--list"]).is_err());

        // `--yes` is the deletion's confirmation; on its own it answers nothing.
        assert!(
            Cli::parse_from(["nix-rebuild", "runs", "abc", "--delete", "--yes"])
                .check_run_arguments()
                .is_ok()
        );
        assert!(
            Cli::parse_from(["nix-rebuild", "runs", "abc", "--yes"])
                .check_run_arguments()
                .is_err()
        );

        // A deletion names a run, and only `runs` deletes.
        assert!(
            Cli::parse_from(["nix-rebuild", "runs", "--delete"])
                .check_run_arguments()
                .is_err()
        );
        assert!(
            Cli::parse_from(["nix-rebuild", "home", "--delete"])
                .check_run_arguments()
                .is_err()
        );
    }

    #[test]
    fn a_resumed_run_rejects_the_flags_that_only_rebuild() {
        use crate::common::cli::Cli;
        use clap::Parser;

        for args in [
            vec!["nix-rebuild", "runs", "--boot"],
            vec!["nix-rebuild", "runs", "--yes"],
            vec!["nix-rebuild", "runs", "--no-clean"],
            vec!["nix-rebuild", "runs", "--flake", "/repo"],
        ] {
            let cli = Cli::parse_from(args.iter().copied());
            assert!(
                cli.check_run_arguments().is_err(),
                "{args:?} changes a rebuild, so a recorded run cannot take it"
            );
        }
    }

    #[test]
    fn scope_parser_accepts_only_home_and_system() {
        assert_eq!(Scope::from_str("home").unwrap(), Scope::Home);
        assert_eq!(Scope::from_str("system").unwrap(), Scope::System);
        assert!(Scope::from_str("other").is_err());
    }
}
