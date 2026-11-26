use super::*;

#[test]
fn root_comparison_and_unborn_history_are_supported() {
    let fixture = Fixture::new(&[]);
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let TaskValue::History(page) = inspect(
        &service,
        &repo,
        Inspection::History(HistoryPage::new(0, 10).expect("page")),
    ) else {
        panic!("expected page");
    };
    assert!(page.root.is_none());
    fixture.write("example", "one\ntwo\n");
    let root = fixture.commit("root");
    let TaskValue::CommitDetails(details) = inspect(
        &service,
        &repo,
        Inspection::CommitDetails(root.clone(), None),
    ) else {
        panic!("expected details");
    };
    assert!(details.parent.is_none());
    assert_eq!(details.files[0].lines, Some((2, 0)));
    let TaskValue::Change(patch) = inspect(
        &service,
        &repo,
        Inspection::CommitPatch(root, None, RepoPath::new("example".into()).expect("path")),
    ) else {
        panic!("expected patch");
    };
    assert!(patch.display().text.contains("+two"));
}
