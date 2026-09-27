//! Local evaluation entry. Configuration and offline debug never call live models.
use hexagon_core::api::Workbench;
use std::{collections::BTreeMap, error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [op, host, config] if op == "freeze" => {
            let request = serde_json::from_slice(&std::fs::read(config)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}", serde_json::to_string_pretty(&wb.freeze_evaluation(&request, None)?)?);
        }
        [op, host, config, parent] if op == "freeze" => {
            let request = serde_json::from_slice(&std::fs::read(config)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}", serde_json::to_string_pretty(&wb.freeze_evaluation(&request, Some(parent))?)?);
        }
        [op, host, id] if op == "batch" => {
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}", serde_json::to_string_pretty(&wb.evaluation_batch(id)?)?);
        }
        [op, host, id, config] if op == "check-config" => {
            let request = serde_json::from_slice(&std::fs::read(config)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            let batch = wb.check_evaluation_configuration(id, &request)?;
            println!("{}", serde_json::to_string_pretty(&batch)?);
            if !batch.ready { return Err("evaluation admission blocked; see structured reasons".into()); }
        }
        [op, host, id, observation] if op == "record-verification" => {
            let observation = serde_json::from_slice(&std::fs::read(observation)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}", serde_json::to_string_pretty(&wb.record_evaluation_verification(id, &observation)?)?);
        }
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
        _ => return Err("usage: evaluate debug HOST TASK.json WRITES.json | read HOST RUN_ID | check-category HOST CORPUS.json | freeze HOST CONFIG.json [PARENT_BATCH] | batch HOST BATCH_ID | check-config HOST BATCH_ID CONFIG.json | record-verification HOST BATCH_ID OBSERVATION.json; live evaluation is not enabled".into()),
    }
    Ok(())
}
