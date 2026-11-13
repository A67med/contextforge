//! Uninstalling on a machine without any agent configuration must not create files.

use contextforge_integrations::{Action, Env, Provider, Scope, plan};

#[test]
fn uninstall_on_clean_machine_creates_nothing() {
    let root = tempfile::tempdir().unwrap();
    let env = Env::for_test(root.path());
    let exe = root.path().join("contextforge.exe");
    for p in Provider::ALL {
        let pl = plan(p, Action::Uninstall, &Scope::User, &exe, &env).unwrap();
        assert!(pl.changes.is_empty(), "{p}: {:?}", pl.changes.iter().map(|c| &c.path).collect::<Vec<_>>());
        assert!(pl.commands.is_empty());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
