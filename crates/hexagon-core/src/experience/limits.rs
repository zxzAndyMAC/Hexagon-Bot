//! Governance 07: Unicode scalar budgets apply to complete rendered entries.
use super::ExperienceEntry;
use crate::{db::Db, proposals::PropError, tools::ToolContext};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export, export_to = "../../../ui/src/gen/")]
pub struct ExperienceLimits {
    pub entry_chars: u32,
    pub load_count: u32,
    pub load_chars: u32,
}
impl Default for ExperienceLimits {
    fn default() -> Self {
        Self {
            entry_chars: 2000,
            load_count: 10,
            load_chars: 8000,
        }
    }
}
pub fn read(db: &Db, project: &str) -> Result<ExperienceLimits, PropError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT entry_chars,load_count,load_chars FROM experience_limits WHERE project_id=?1",
            [project],
            |r| {
                Ok(ExperienceLimits {
                    entry_chars: r.get(0)?,
                    load_count: r.get(1)?,
                    load_chars: r.get(2)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}
pub fn set(
    db: &Db,
    ctx: &ToolContext,
    limits: &ExperienceLimits,
) -> Result<ExperienceLimits, PropError> {
    let started = std::time::Instant::now();
    let valid = ctx.agent_id == "owner"
        && limits.entry_chars > 0
        && limits.load_count > 0
        && limits
            .entry_chars
            .checked_add(128)
            .is_some_and(|minimum| limits.load_chars >= minimum);
    crate::diag::note(
        if valid {
            crate::diag::CLASS_JUDGE
        } else {
            crate::diag::CLASS_REJECT
        },
        !valid,
        Some(&ctx.project_id),
        Some(&ctx.agent_id),
        None,
        None,
        "experience_limits",
        if valid {
            "owner_configured"
        } else {
            "invalid_or_unauthorized"
        },
        started,
    );
    if !valid {
        return Err(PropError::Rejected("experience limits require owner, positive integers and 128 characters of rendering overhead".into()));
    }
    let _lease = if ctx.write_lease.is_none() {
        Some(crate::tools::writeguard::repository_lock(ctx)?)
    } else {
        None
    };
    db.conn().execute("INSERT INTO experience_limits(project_id,entry_chars,load_count,load_chars) VALUES(?1,?2,?3,?4) ON CONFLICT(project_id) DO UPDATE SET entry_chars=excluded.entry_chars,load_count=excluded.load_count,load_chars=excluded.load_chars", params![ctx.project_id,limits.entry_chars,limits.load_count,limits.load_chars])?;
    Ok(limits.clone())
}
pub(super) fn entry_size(
    body: &str,
    notes: &str,
    conditions: &super::ExperienceConditions,
) -> usize {
    body.chars().count()
        + notes.chars().count()
        + serde_json::to_string(conditions)
            .expect("string-only conditions serialize")
            .chars()
            .count()
}
pub(super) fn render(entries: Vec<ExperienceEntry>, limits: &ExperienceLimits) -> String {
    plan(entries, limits).0
}
pub(super) fn plan(
    mut entries: Vec<ExperienceEntry>,
    limits: &ExperienceLimits,
) -> (String, std::collections::HashSet<String>) {
    fn dimensions(e: &ExperienceEntry) -> usize {
        [
            &e.conditions.roles,
            &e.conditions.stages,
            &e.conditions.paths,
        ]
        .iter()
        .filter(|v| !v.is_empty())
        .count()
    }
    entries.sort_by(|a, b| {
        dimensions(b)
            .cmp(&dimensions(a))
            .then(a.entry_id.cmp(&b.entry_id))
    });
    let rendered: Vec<Option<String>> = entries
        .iter()
        .map(|e| {
            if entry_size(&e.body, &e.notes, &e.conditions) > limits.entry_chars as usize {
                return None;
            }
            Some(format!(
                "\n[Experience {}]\n{}\n{}\nConditions: {}\n",
                e.entry_id,
                e.body,
                e.notes,
                serde_json::to_string(&e.conditions).expect("string conditions")
            ))
        })
        .collect();
    let notice = |omitted: usize| format!("\n[Experience omitted by limits: {omitted}]\n");
    // If everything fits there is no omission overhead. Otherwise reserve the
    // largest possible count so a late omission can never overflow the budget.
    let all_fit = rendered.len() <= limits.load_count as usize
        && rendered.iter().all(Option::is_some)
        && rendered
            .iter()
            .flatten()
            .map(|s| s.chars().count())
            .sum::<usize>()
            <= limits.load_chars as usize;
    let reserve = if all_fit {
        0
    } else {
        notice(rendered.len()).chars().count()
    };
    let mut result = String::new();
    let mut used = reserve;
    let mut count = 0;
    let mut omitted = 0;
    let mut selected = std::collections::HashSet::new();
    for (index, value) in rendered.into_iter().enumerate() {
        if let Some(value) = value {
            let size = value.chars().count();
            if count < limits.load_count && used + size <= limits.load_chars as usize {
                selected.insert(entries[index].entry_id.clone());
                result.push_str(&value);
                used += size;
                count += 1;
                continue;
            }
        }
        omitted += 1;
    }
    if omitted > 0 {
        result.push_str(&notice(omitted));
    }
    (result, selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn whole_entries_and_notices_stay_within_both_budgets(length in 1usize..500, maximum in 200u32..1000, count in 1u32..8) {
            let entries = (0..12).map(|n|ExperienceEntry { schema_version:1,entry_id:format!("e{n:02}"),revision:1,project_id:"p".into(),skill:"s".into(),body:"😀".repeat(length),notes:String::new(),conditions:Default::default(),sources:vec![] }).collect();
            let limits = ExperienceLimits { entry_chars: maximum - 128, load_count:count, load_chars:maximum };
            let text = render(entries, &limits);
            prop_assert!(text.chars().count() <= maximum as usize);
            let loaded = text.matches("[Experience e").count();
            prop_assert!(loaded <= count as usize);
            prop_assert_eq!(text.matches('😀').count(), loaded * length);
        }
    }
}
