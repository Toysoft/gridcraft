use gridcraft_engine::Session;
use serde_json::json;

const NOT_OBJECTS: [&str; 4] = ["\"[1,2]\"", "\"7\"", "\"true\"", "\"\"\"hi\"\"\""];

fn with_name(name: &str, refers_to: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("formulas.defineName", json!({"name": name, "refersTo": refers_to})).unwrap();
    s
}

#[test]
fn saving_a_view_works_when_stored_list_is_not_an_object() {
    for text in NOT_OBJECTS {
        let mut s = with_name("_sc_custom_views", text);
        let out = s.execute("view.customViews", json!({"save": "v"})).unwrap();
        assert!(out.get("v").is_some(), "{text}: {out}");
    }
}

#[test]
fn saving_a_view_keeps_the_keys_of_a_stored_object() {
    let mut s = with_name("_sc_custom_views", "\"{\"\"a\"\":1}\"");
    let out = s.execute("view.customViews", json!({"save": "v"})).unwrap();
    assert!(out.get("a").is_some() && out.get("v").is_some(), "{out}");
}

/// Scripts of earlier versions lived in a hidden name (`_sc_scripts`); opening such a file moves
/// them into the workbook's scripts. A stored value that isn't the expected shape is dropped, not
/// an error.
#[test]
fn legacy_script_lists_of_any_shape_open() {
    for text in NOT_OBJECTS.iter().chain(&["\"{\"\"a\"\":5,\"\"b\"\":[]}\""]) {
        let s = with_name("_sc_scripts", text);
        let bytes = gridcraft_engine::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
        let (wb, _) = gridcraft_engine::io::open_bytes("book.xlsx", &bytes).unwrap();
        assert!(!wb.names.iter().any(|n| n.name == "_sc_scripts"), "{text}");
        assert!(wb.scripts.iter().all(|sc| sc.name == "b"), "{text}: {:?}", wb.scripts);
    }
}

#[test]
fn recording_and_creating_scripts_ignore_a_stray_legacy_name() {
    let mut s = with_name("_sc_scripts", "\"[1,2]\"");
    s.execute("automate.newScript", json!({"name": "s1", "commands": []})).unwrap();
    s.execute("automate.recordActions", json!({})).unwrap();
    s.execute("automate.recordActions", json!({"stop": true, "name": "s2"})).unwrap();
    let list = s.execute("automate.allScripts", json!({})).unwrap();
    assert!(list.to_string().contains("s1") && list.to_string().contains("s2"), "{list}");
}
