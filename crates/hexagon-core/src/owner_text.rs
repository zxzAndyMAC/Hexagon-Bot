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

// ---------- 失速监视的工作台注记（stall-watch 票 02–04 / ADR 0074） ----------
// 按钮名与界面 i18n `cards.stallRetry` / `cards.stallAck` 同名。

pub fn stall_no_reply_card(role: &str) -> String {
    match code() {
        "zh-CN" => format!("{role} 重触发一次后仍没有可见回复，已交给负责人。"),
        "zh-TW" => format!("{role} 重新觸發一次後仍沒有可見回覆，已交給負責人。"),
        "ja" => {
            format!("{role} は一度再起動しても見える返信がありません。オーナーに引き渡しました。")
        }
        "es" => {
            format!("{role} sigue sin respuesta visible tras un reintento. Se pasa al responsable.")
        }
        "pt" => format!(
            "{role} continua sem resposta visível após uma nova tentativa. Passado ao responsável."
        ),
        "fr" => {
            format!("{role} reste sans réponse visible après une relance. Transmis au responsable.")
        }
        _ => format!("{role} still has no visible reply after one retrigger. Handed to the owner."),
    }
}

pub fn stall_idle_no_pm() -> &'static str {
    match code() {
        "zh-CN" => "有回复但进度没有变化（阶段指针、产物、待决卡都没动），花名册上没有项目经理可以调查，已交给负责人。",
        "zh-TW" => "有回覆但進度沒有變化（階段指標、產物、待決卡都沒動），名冊上沒有專案經理可以調查，已交給負責人。",
        "ja" => "返信はあるものの進捗がありません（段階ポインタ・成果物・保留カードすべて不変）。調べるプロジェクトマネージャがいないため、オーナーに引き渡しました。",
        "es" => "Hubo respuesta pero nada avanzó (puntero de etapa, artefactos y tarjetas sin cambios). No hay director de proyecto que investigue. Se pasa al responsable.",
        "pt" => "Houve resposta, mas nada avançou (ponteiro de etapa, artefatos e cartões sem mudança). Não há gerente de projeto para investigar. Passado ao responsável.",
        "fr" => "Il y a eu une réponse mais rien n'a avancé (pointeur d'étape, artefacts et cartes inchangés). Aucun chef de projet pour enquêter. Transmis au responsable.",
        _ => "There was a reply but nothing moved (stage pointer, artifacts and pending cards unchanged). No project manager is on the roster to investigate. Handed to the owner.",
    }
}

pub fn stall_idle_after_investigation() -> &'static str {
    match code() {
        "zh-CN" => "项目经理派活之后进度仍没有变化，已交给负责人。",
        "zh-TW" => "專案經理派工之後進度仍沒有變化，已交給負責人。",
        "ja" => "プロジェクトマネージャが割り振った後も進捗がありません。オーナーに引き渡しました。",
        "es" => "Tras el reparto del director de proyecto sigue sin haber avance. Se pasa al responsable.",
        "pt" => "Depois da distribuição do gerente de projeto ainda não houve avanço. Passado ao responsável.",
        "fr" => "Après l'attribution du chef de projet, toujours aucun avancement. Transmis au responsable.",
        _ => "Still no progress after the project manager dispatched. Handed to the owner.",
    }
}

pub fn stall_investigation_timeout() -> &'static str {
    match code() {
        "zh-CN" => "项目经理这轮调查没有给出封闭选择，已交给负责人。",
        "zh-TW" => "專案經理這輪調查沒有給出封閉選擇，已交給負責人。",
        "ja" => "プロジェクトマネージャの調査は閉じた選択を返しませんでした。オーナーに引き渡しました。",
        "es" => "La investigación del director de proyecto no dio una elección cerrada. Se pasa al responsable.",
        "pt" => "A investigação do gerente de projeto não deu uma escolha fechada. Passado ao responsável.",
        "fr" => "L'enquête du chef de projet n'a pas donné de choix fermé. Transmis au responsable.",
        _ => "The project manager's investigation gave no closed choice. Handed to the owner.",
    }
}

pub fn stall_hold_closed() -> &'static str {
    match code() {
        "zh-CN" => "项目经理调查后选择先不派活，这次失速收场。有新的负责人消息或新的激活再重新计时。",
        "zh-TW" => "專案經理調查後選擇先不派工，這次失速收場。有新的負責人訊息或新的啟用再重新計時。",
        "ja" => "プロジェクトマネージャは調査の結果、今は割り振らないことにしました。この停滞は終了です。オーナーの新しいメッセージか新しい起動で再び計測します。",
        "es" => "Tras investigar, el director de proyecto decidió no repartir por ahora. Este atasco se cierra; se vuelve a medir con un nuevo mensaje del responsable o una nueva activación.",
        "pt" => "Após investigar, o gerente de projeto decidiu não distribuir por enquanto. Este travamento se encerra; volta a contar com nova mensagem do responsável ou nova ativação.",
        "fr" => "Après enquête, le chef de projet a choisi de ne rien attribuer pour l'instant. Ce blocage est clos ; le décompte reprend à un nouveau message du responsable ou une nouvelle activation.",
        _ => "After investigating, the project manager chose to wake no one for now. This stall is closed; timing restarts on a new owner message or a new activation.",
    }
}

pub fn stall_ack_closed() -> &'static str {
    match code() {
        "zh-CN" => "负责人点了「知道了」，这次失速收场。有新的负责人消息或新的激活再重新计时。",
        "zh-TW" => "負責人點了「知道了」，這次失速收場。有新的負責人訊息或新的啟用再重新計時。",
        "ja" => "オーナーが「了解」を押しました。この停滞は終了です。オーナーの新しいメッセージか新しい起動で再び計測します。",
        "es" => "El responsable pulsó «Entendido». Este atasco se cierra; se vuelve a medir con un nuevo mensaje del responsable o una nueva activación.",
        "pt" => "O responsável clicou em «Entendi». Este travamento se encerra; volta a contar com nova mensagem do responsável ou nova ativação.",
        "fr" => "Le responsable a cliqué sur « Compris ». Ce blocage est clos ; le décompte reprend à un nouveau message du responsable ou une nouvelle activation.",
        _ => "The owner clicked \"Got it\". This stall is closed; timing restarts on a new owner message or a new activation.",
    }
}
