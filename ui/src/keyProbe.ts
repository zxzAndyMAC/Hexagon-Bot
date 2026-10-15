// 向导服务商步实时密钥检测（gap B4，entry#wz-provider「检测通过 · 200K 窗口」）。
// 输入停 600ms 后检测一次；更晚的输入作废更早的结果（序号守卫），不让旧回应盖掉新输入。
// 检测只是建议，不是放行闸：放行仍看 providerStepReady（default 槽已启用且已存钥匙）。
import { useEffect, useRef, useState } from 'react'
import { api, errText, type ProviderView } from './api'
import type { KeyProbe } from './gen/KeyProbe'

export const PROBE_DEBOUNCE_MS = 600

export type ProbeState =
  | { phase: 'idle' }
  | { phase: 'running' }
  | { phase: 'done'; result: KeyProbe }
  /** 命令本身失败（不是服务商的回应），原因用错误文案。 */
  | { phase: 'error'; message: string }

type ProbeDef = Pick<ProviderView, 'id' | 'name' | 'kind' | 'base_url' | 'models' | 'enabled'>

/** `def` 为 null 表示条件不全（缺地址 / 钥匙 / 模型），不检测。 */
export function useKeyProbe(def: ProbeDef | null, secret: string | undefined, model: string): ProbeState {
  // 状态带上它所属的输入；输入一变，旧结果即不显示（防抖期间回到 idle）。
  const [state, setState] = useState<{ key: string; probe: ProbeState }>({ key: '', probe: { phase: 'idle' } })
  const seq = useRef(0)
  const key = def ? JSON.stringify([def.id, def.kind, def.base_url, secret ?? '', model.trim()]) : ''
  useEffect(() => {
    const mine = ++seq.current
    if (!key || !def) return
    const timer = setTimeout(() => {
      if (mine !== seq.current) return
      setState({ key, probe: { phase: 'running' } })
      api.probeProviderKey({ id: def.id, name: def.name, kind: def.kind, base_url: def.base_url, models: [], enabled: def.enabled }, secret, model.trim())
        .then((result) => { if (mine === seq.current) setState({ key, probe: { phase: 'done', result } }) })
        .catch((e) => { if (mine === seq.current) setState({ key, probe: { phase: 'error', message: errText(e) } }) })
    }, PROBE_DEBOUNCE_MS)
    return () => clearTimeout(timer)
    // key 已覆盖 def / secret / model 中参与检测的字段
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return key && state.key === key ? state.probe : { phase: 'idle' }
}

/** 窗口紧凑写法：200000 → 200K，1000000 → 1M。 */
export function fmtWindow(n: number): string {
  if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`
  return `${Math.round(n / 1_000)}K`
}
