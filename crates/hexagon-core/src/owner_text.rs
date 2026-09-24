//! 给负责人看的固定句（ADR 0073）。
//!
//! 时间线注记在写下时按界面语言拼进消息正文，之后不改写。未设置回落英文。
//! 七种语言手写在这里：句子短、要确定，不走翻译槽。
//! 卡上的判定理由、提案警告、溯源不在这里——那些是原因码，界面渲染。

fn code() -> &'static str {
    crate::uilang::interface_code()
}

pub fn unknown() -> &'static str {
    match code() {
        "zh-CN" | "zh-TW" => "未知",
        "ja" => "不明",
        "es" => "desconocido",
        "pt" => "desconhecido",
        "fr" => "inconnu",
        _ => "unknown",
    }
}

/// 命令块里多条命令之间的分隔。
pub fn list_sep() -> &'static str {
    match code() {
        "zh-CN" | "zh-TW" | "ja" => "；",
        _ => "; ",
    }
}

pub fn no_receiver_note() -> &'static str {
    match code() {
        "zh-CN" => "没有接话的人。项目经理未勾选；流程包要当前阶段激活名单的第一位，快速通道要通道角色，这里都没有。",
        "zh-TW" => "沒有接話的人。專案經理未勾選；流程包要目前階段啟用名單的第一位，快速通道要通道角色，這裡都沒有。",
        "ja" => "受け取る人がいません。プロジェクトマネージャは選ばれておらず、プロセスパックは現在の段階の先頭、短経路はその役割を要しますが、どちらもありません。",
        "es" => "Nadie recibe el turno. El director de proyecto no está seleccionado; el paquete de proceso pide el primer rol de la etapa, y el atajo pide el rol del atajo. No hay ninguno.",
        "pt" => "Ninguém recebe a vez. O gerente de projeto não está selecionado; o pacote de processo pede o primeiro papel da etapa, e o atalho pede o papel do atalho. Não há nenhum.",
        "fr" => "Personne ne prend le relais. Le chef de projet n'est pas sélectionné ; le paquet de processus demande le premier rôle de l'étape, et le raccourci demande le rôle du raccourci. Il n'y en a aucun.",
        _ => "Nobody is taking this. The project manager is not selected; a process pack needs the first role of the current stage, and a fast path needs the fast-path role. None of those is here.",
    }
}

pub fn no_intake_speaker() -> &'static str {
    match code() {
        "zh-CN" => "没有接话的人做这次开场分析。项目经理未勾选；流程包要当前阶段激活名单的第一位，快速通道要通道角色，这里都没有。",
        "zh-TW" => "沒有接話的人做這次開場分析。專案經理未勾選；流程包要目前階段啟用名單的第一位，快速通道要通道角色，這裡都沒有。",
        "ja" => "この開場分析を受け取る人がいません。プロジェクトマネージャは選ばれておらず、プロセスパックは現在の段階の先頭、短経路はその役割を要しますが、どちらもありません。",
        "es" => "Nadie hace este análisis de apertura. El director de proyecto no está seleccionado; el paquete de proceso pide el primer rol de la etapa, y el atajo pide el rol del atajo. No hay ninguno.",
        "pt" => "Ninguém faz esta análise de abertura. O gerente de projeto não está selecionado; o pacote de processo pede o primeiro papel da etapa, e o atalho pede o papel do atalho. Não há nenhum.",
        "fr" => "Personne ne fait cette analyse d'ouverture. Le chef de projet n'est pas sélectionné ; le paquet de processus demande le premier rôle de l'étape, et le raccourci demande le rôle du raccourci. Il n'y en a aucun.",
        _ => "Nobody is here to write this opening analysis. The project manager is not selected; a process pack needs the first role of the current stage, and a fast path needs the fast-path role. None of those is here.",
    }
}

pub fn existing_instructions(file: &str) -> String {
    match code() {
        "zh-CN" => format!("\n已有项目说明：{file}（只引用，不覆盖）\n"),
        "zh-TW" => format!("\n已有專案說明：{file}（只引用，不覆蓋）\n"),
        "ja" => format!("\n既存のプロジェクト説明：{file}（参照のみ、上書きしない）\n"),
        "es" => format!(
            "\nInstrucciones de proyecto ya existentes: {file} (solo citar, no sobrescribir)\n"
        ),
        "pt" => format!(
            "\nInstruções de projeto já existentes: {file} (apenas citar, não sobrescrever)\n"
        ),
        "fr" => format!(
            "\nInstructions de projet déjà présentes : {file} (citer seulement, ne pas écraser)\n"
        ),
        _ => format!("\nExisting project instructions: {file} (cite only, do not overwrite)\n"),
    }
}

pub fn draft_banner() -> &'static str {
    match code() {
        "zh-CN" => "\n## 草案\n确认后才写入 AGENTS.md。\n\n",
        "zh-TW" => "\n## 草案\n確認後才寫入 AGENTS.md。\n\n",
        "ja" => "\n## 草案\n確認後にのみ AGENTS.md へ書き込みます。\n\n",
        "es" => "\n## Borrador\nSe escribe en AGENTS.md solo después de confirmar.\n\n",
        "pt" => "\n## Rascunho\nSó é gravado em AGENTS.md depois de confirmar.\n\n",
        "fr" => "\n## Brouillon\nÉcrit dans AGENTS.md seulement après confirmation.\n\n",
        _ => "\n## Draft\nWritten to AGENTS.md only after you confirm.\n\n",
    }
}
