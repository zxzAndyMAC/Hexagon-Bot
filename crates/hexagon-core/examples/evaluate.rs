//! Local evaluation entry. Configuration and offline debug never call live models.
use hexagon_core::api::Workbench;
use std::{collections::BTreeMap, error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [op, host, batch] if op == "generation" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.prepare_evaluation_generation(batch)?)?);
        }
        [op, host, id] if op == "read-generation" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluation_generation(id)?)?);
        }
        [op, host, id, task] if op == "reveal-task" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.reveal_evaluation_task(id,task)?)?);
        }
        [op, host] if op == "check-isolation" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            let report=wb.check_evaluation_isolation()?;
            println!("{}",serde_json::to_string_pretty(&report)?);
            if !report.passed {return Err("isolation not verified".into());}
        }
        [op, host, id] if op == "read-isolation" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluation_isolation_check(id)?)?);
        }
        [op, host, plan] if op == "stop-plan" => {
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.stop_evaluation_plan(plan)?)?);
        }
        [op, host, batch, kind] if op == "plan" => {
            let kind = match kind.as_str() { "pilot" => hexagon_core::evaluation::PlanKind::Pilot, "formal" => hexagon_core::evaluation::PlanKind::Formal, _ => return Err("plan kind must be pilot or formal".into()) };
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.plan_evaluation(batch,kind)?)?);
        }
        [op, host, plan] if op == "read-plan" => {
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluation_plan(plan)?)?);
        }
        [op, host, plan, script] if op == "next-debug" => {
            let activations: Vec<hexagon_core::evaluation::DebugActivation> = serde_json::from_slice(&std::fs::read(script)?)?;
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluate_next_debug(plan,&activations)?)?);
        }
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
        _ => return Err("usage: evaluate debug HOST TASK.json WRITES.json | read HOST RUN_ID | check-category HOST CORPUS.json | freeze HOST CONFIG.json [PARENT_BATCH] | batch HOST BATCH_ID | check-config HOST BATCH_ID CONFIG.json | record-verification HOST BATCH_ID OBSERVATION.json | plan HOST BATCH_ID pilot|formal | read-plan HOST PLAN_ID | next-debug HOST PLAN_ID SCRIPT.json | stop-plan HOST PLAN_ID | generation HOST BATCH_ID | read-generation HOST CONTEXT_ID | reveal-task HOST CONTEXT_ID TASK_ID | check-isolation HOST | read-isolation HOST CHECK_ID; live evaluation is not enabled".into()),
    }
    Ok(())
}
