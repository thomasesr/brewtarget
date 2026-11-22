#[test]
fn probe2() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for f in ["DefaultContent003-Ingredients-Hops-Yeasts.json", "DefaultContent002-BJCP_2021_Styles.json"] {
        let p = repo.join("data").join(f);
        let v = bt_format::jsonc::value_from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        match bt_format::document::read_beerjson(&v) {
            Ok(doc) => {
                println!("{f}: {} records", doc.len());
                for (c, rs) in &doc.collections {
                    if !rs.is_empty() {
                        println!("   {c}: {} -> table {}", rs.len(), rs[0].record.table);
                    }
                }
                if let Some((_, rs)) = doc.collections.iter().find(|(c, _)| c == "hop") {
                    let mut cols: Vec<_> = rs[0].record.columns.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    cols.sort();
                    println!("   first hop: {}", cols.join(", "));
                }
            }
            Err(e) => println!("{f}: ERROR {e}"),
        }
    }
}
