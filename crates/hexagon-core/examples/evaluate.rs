//! Local evaluation entry. Configuration and offline debug never call live models.
use hexagon_core::api::Workbench;
use std::{collections::BTreeMap, error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [op,host,run] if matches!(op.as_str(),"control"|"stop-run"|"pause-run"|"resume-run") => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            let state=match op.as_str() {"stop-run"=>wb.stop_evaluation_run(run)?,"pause-run"=>wb.pause_evaluation_run(run)?,"resume-run"=>wb.resume_evaluation_run(run)?,_=>wb.evaluation_control(run)?};
            println!("{}",serde_json::to_string_pretty(&state)?);
        }

        [command, host, plan, price] if command == "debug-budget-config" => {
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            wb.enable_evaluation_budget_debug(plan, serde_json::from_slice(&std::fs::read(price)?)?)?;
            println!("{}", serde_json::to_string_pretty(&wb.evaluation_budget_debug()?)?);
        }
        [command, host] if command == "budget-debug" || command == "budget" => {
            let wb = Workbench::open_evaluation_host(Path::new(host))?;
            let report = if command == "budget-debug" { wb.evaluation_budget_debug()? } else { wb.evaluation_budget()? };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        [command, host, run] if command == "outcome" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.inspect_evaluation_outcome(run)?)?);
        }
        [command, host, run] if command == "outcome-history" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluation_outcome_history(run)?)?);
        }
        [command, host, run] if command == "recheck" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.recheck_evaluation_delivery(run)?)?);
        }
        [op,host,run] if op=="pending" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluation_pending(run)?)?);
        }

        [op,host,plan,script] if op=="next-owner-debug" => {
            let acts=serde_json::from_slice::<Vec<hexagon_core::evaluation::DebugActivation>>(&std::fs::read(script)?)?;
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluate_next_with_owner_debug(plan,&acts)?)?);
        }
        [op,host,run] if op=="owner-session" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;owner_session(&wb,run)?;
        }
        [op,host,run] if op=="timing" => {
            let wb=Workbench::open_evaluation_host(Path::new(host))?;
            println!("{}",serde_json::to_string_pretty(&wb.evaluation_timing(run)?)?);
        }

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
        _ => return Err("usage: evaluate debug HOST TASK.json WRITES.json | read HOST RUN_ID | check-category HOST CORPUS.json | freeze HOST CONFIG.json [PARENT_BATCH] | batch HOST BATCH_ID | check-config HOST BATCH_ID CONFIG.json | record-verification HOST BATCH_ID OBSERVATION.json | plan HOST BATCH_ID pilot|formal | read-plan HOST PLAN_ID | next-debug HOST PLAN_ID SCRIPT.json | stop-plan HOST PLAN_ID | generation HOST BATCH_ID | read-generation HOST CONTEXT_ID | reveal-task HOST CONTEXT_ID TASK_ID | check-isolation HOST | read-isolation HOST CHECK_ID | next-owner-debug HOST PLAN_ID SCRIPT.json | owner-session HOST RUN_ID | timing HOST RUN_ID | pending HOST RUN_ID | outcome HOST RUN_ID | outcome-history HOST RUN_ID | recheck HOST RUN_ID | debug-budget-config HOST PLAN_ID PRICE.json | budget-debug HOST | budget HOST | control HOST RUN_ID | stop-run HOST RUN_ID | pause-run HOST RUN_ID | resume-run HOST RUN_ID; live evaluation is not enabled".into()),
    }
    Ok(())
}

fn owner_session(wb: &Workbench, run_id: &str) -> Result<(), Box<dyn Error>> {
    use hexagon_core::evaluation::{
        AttentionEnd, AttentionHandle, EvaluationActor, EvaluationDecision,
    };
    use std::io::{BufRead, IsTerminal};
    let input = std::io::stdin();
    let actor = if input.is_terminal() {
        EvaluationActor::Human
    } else {
        EvaluationActor::Scripted
    };
    let mut handle: Option<AttentionHandle> = None;
    eprintln!("start/continue | guidance TEXT | approve | decision JSON | away | timing | quit. Edit the printed workspace only inside a started interval. Noninteractive input is scripted evidence.");
    println!(
        "{}",
        serde_json::to_string_pretty(&wb.evaluation_result(run_id)?)?
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&wb.evaluation_pending(run_id)?)?
    );
    for line in input.lock().lines() {
        let line = line?;
        let command = line.trim();
        let result = (|| -> Result<bool, Box<dyn Error>> {
            match command {
                "start" | "continue" => {
                    if handle.is_some() {
                        return Err("attention already active".into());
                    }
                    handle = Some(wb.begin_evaluation_attention(run_id, actor)?);
                    eprintln!("attention started");
                }
                "away" => {
                    let h = handle.as_ref().ok_or("start attention first")?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(
                            &wb.end_evaluation_attention(h, AttentionEnd::Away)?
                        )?
                    );
                    handle = None;
                }
                "timing" => println!(
                    "{}",
                    serde_json::to_string_pretty(&wb.evaluation_timing(run_id)?)?
                ),
                "quit" => return Ok(true),
                _ if command.starts_with("guidance ") => wb.add_evaluation_guidance(
                    handle.as_ref().ok_or("start attention first")?,
                    &command[9..],
                )?,
                _ => {
                    let decision = if command == "approve" {
                        EvaluationDecision::ApproveStamp
                    } else if let Some(json) = command.strip_prefix("decision ") {
                        serde_json::from_str(json)?
                    } else {
                        return Err("unknown command".into());
                    };
                    let run = wb.submit_evaluation_decision(
                        handle.as_ref().ok_or("start attention first")?,
                        decision,
                    )?;
                    handle = None;
                    println!("{}", serde_json::to_string_pretty(&run)?);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&wb.evaluation_pending(run_id)?)?
                    );
                    if run.state != "waiting_human" {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })();
        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(error) => {
                eprintln!("{error}");
                if let Some(h) = &handle {
                    if wb
                        .evaluation_timing(run_id)?
                        .intervals
                        .iter()
                        .any(|i| i.id == h.id() && i.ended_at_ms.is_some())
                    {
                        handle = None;
                        eprintln!("Start a new attention interval to continue.");
                    }
                }
            }
        }
    }
    if let Some(h) = handle {
        wb.end_evaluation_attention(&h, AttentionEnd::Away)?;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&wb.evaluation_timing(run_id)?)?
    );
    Ok(())
}
