use super::*;

pub(super) fn write(
    anchor: HistoryAnchor,
    newest: &HistoryPage,
    older: &HistoryPage,
    current: &HistoryDetail,
    first: &HistoryDetail,
) {
    let Some(directory) = std::env::var_os("AGENT_HISTORY_BROWSER_FIXTURES") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    for (name, html) in [
        (
            "loader.html",
            crate::adapters::http::pages::agent_history_loader(anchor),
        ),
        ("newest.html", agent_history_section(anchor, newest)),
        ("older.html", agent_history_section(anchor, older)),
        ("current.html", agent_history_detail(anchor, current)),
        ("first.html", agent_history_detail(anchor, first)),
    ] {
        std::fs::write(directory.join(name), html).unwrap();
    }
    std::fs::write(directory.join("facts.json"),serde_json::to_vec(&json!({
        "region":anchor.region(),"latest_order":newest.entries.first().unwrap().order,
        "oldest_order":older.entries.last().unwrap().order,"first_output":first.payload.as_ref().unwrap().output,
        "source":"native max-page history test, actual production HTML renderers"
    })).unwrap()).unwrap();
}

pub(super) fn write_fragment(anchor: HistoryAnchor, name: &str, detail: &HistoryDetail) {
    let Some(directory) = std::env::var_os("AGENT_HISTORY_BROWSER_FIXTURES") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(name), agent_history_detail(anchor, detail)).unwrap();
}
