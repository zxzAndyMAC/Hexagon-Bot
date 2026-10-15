// 阶段操作统一入口（ui-audit 票 03，ADR 0056 确认分级）：
// StageBar 按钮、keymap 绑定、⌘K 指令三路共用同一个 runStageOp。
// L1 可逆（pause/resume/stamp）直执行；sleepAll 自票 02 起走非 danger 确认（见下）；
// L2 流程破坏（rewind=run 置 rejected+重开、skip）过应用内确认层。
// 出处：倒挂现场——可逆的 sleepAll 弹原生 confirm、rewind 裸按钮（report P1-8）。
import { api, errText } from './api'
import { useUiStore } from './store'
import i18n from './i18n'

export type StageOp = 'rewind' | 'skip' | 'stamp' | 'pause' | 'resume' | 'sleepAll'

/** 执行阶段操作；条件不满足（无活动阶段等）返回 false，已派发返回 true。 */
export function runStageOp(op: StageOp): boolean {
  const st = useUiStore.getState()
  const active = st.stages.find((s) => s.state === 'active' || s.state === 'waiting_stamp')
  const fire = (f: () => Promise<unknown>) => {
    void f()
      .then(() => st.invalidate())
      .catch((e) => st.pushToast(errText(e), 'err'))
  }
  switch (op) {
    case 'rewind': {
      if (!active || active.seq <= 0) return false
      st.askConfirm({
        title: i18n.t('stage.rewindConfirm', { stage: active.stage }),
        danger: true,
        run: () => api.rewind(active.seq - 1),
      })
      return true
    }
    case 'skip': {
      if (!active) return false
      st.askConfirm({
        title: i18n.t('stage.skipConfirm', { stage: active.stage }),
        danger: true,
        run: () => api.skip(),
      })
      return true
    }
    case 'stamp': {
      if (active?.state !== 'waiting_stamp') return false
      fire(() => api.stamp())
      return true
    }
    case 'pause': {
      if (!active) return false
      fire(() => api.pause())
      return true
    }
    case 'resume': {
      fire(() => api.resume())
      return true
    }
    case 'sleepAll': {
      // ui-redesign 票 02（2026-10-13）：全员休眠回到二次确认——规范 components D18「全员休眠等危险指令
      // 需二次确认」、interaction §13 确认层清单、票 02 验收条「全员休眠仍要二次确认」。它仍可逆（团队列表随时
      // 唤醒），所以是非 danger 的安全类确认（A17：首焦主按钮，↵ 直接确认），不是 L2 流程破坏的红色确认。
      // 被改写的旧裁决：ADR 0056 L1「直执行」（修过原生 confirm 的倒挂）。2026-10-14 负责人裁决维持确认
      // （ADR 0056 附录）——不要再改回直执行，也不要换成原生 confirm。
      st.askConfirm({
        title: i18n.t('header.sleepAllConfirm'),
        body: i18n.t('header.sleepAllBody'),
        confirmLabel: i18n.t('header.sleepAll'),
        run: () => api.sleepAll(),
      })
      return true
    }
  }
}
