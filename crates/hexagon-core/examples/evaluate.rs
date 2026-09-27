//! Local evaluation entry. Only offline debug and stored-result reads are enabled.
use hexagon_core::api::Workbench;
use std::{collections::BTreeMap, error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [op, host, task, writes] if op == "debug" => {
            let task = serde_json::from_slice(&std::fs::read(task)?)?;
            let writes: BTreeMap<String, String> = serde_json::from_slice(&std::fs::read(writes)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}", serde_json::to_string_pretty(&wb.evaluate_debug(&task, &writes)?)?);
        }
        [op, host, corpus] if op == "check-category" => {
            let corpus = serde_json::from_slice(&std::fs::read(corpus)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            let report = wb.check_evaluation_category(&corpus)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !report.passed { return Err("fixture self-check failed".into()); }
        }
        [op, host, id] if op == "read" => {
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}", serde_json::to_string_pretty(&wb.evaluation_result(id)?)?);
        }
        _ => return Err("usage: evaluate debug HOST TASK.json WRITES.json | evaluate read HOST RUN_ID; evaluate check-category HOST CORPUS.json; live evaluation is not enabled".into()),
    }
    Ok(())
}
