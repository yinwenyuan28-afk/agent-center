#[test]
fn model_json_always_includes_arrays() {
    let m = crate::providers::matrix::build().expect("build");
    let mut omitted = 0;
    for p in &m.providers {
        for model in &p.models {
            let s = serde_json::to_string(model).unwrap();
            let v: serde_json::Value = serde_json::from_str(&s).unwrap();
            let obj = v.as_object().unwrap();
            for key in ["inputModalities", "outputModalities", "reasoningEfforts"] {
                if !obj.contains_key(key) {
                    omitted += 1;
                    eprintln!("OMITTED {key} in {}/{}: {s}", p.id, model.id);
                }
            }
        }
    }
    assert_eq!(omitted, 0, "empty modality arrays must be present in IPC JSON");
}
